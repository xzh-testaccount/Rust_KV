//! 异步命令行客户端。

use crate::error::{AppError, ErrorCode, Result};
use crate::protocol::{
    Frame, Response, ResponseData, encode_request_line, parse_command, parse_response_line,
    read_frame_async,
};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use tokio::{
    io::{self, AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
};

use crate::{
    error::{AppError, ErrorCode, Result},
    protocol::{
        Frame, Request, Response, ResponseData, encode_request_line, parse_command,
        parse_response_bytes, read_frame_async,
    },
    server::DEFAULT_BIND_ADDRESS,
};

const PROMPT: &str = "kv> ";
const COMMANDS: &str = "commands: set KEY VALUE | get KEY | delete KEY | keys | status | \
storage-status | compact | ping | quit";

/// 客户端启动参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    pub server: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server: DEFAULT_BIND_ADDRESS
                .parse()
                .expect("built-in server address must be valid"),
        }
    }
}

impl ClientConfig {
    /// 解析进程参数；返回 `None` 表示只显示帮助。
    pub fn parse<I, S>(args: I) -> Result<Option<Self>>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = args.into_iter().map(Into::into);
        let _program = args.next();
        parse_client_args(args)
    }
}

pub fn parse_client_args<I, S>(args: I) -> Result<Option<ClientConfig>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut config = ClientConfig::default();
    let mut server_seen = false;
    let mut help_seen = false;
    let mut args = args.into_iter().map(Into::into);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => {
                if help_seen || server_seen {
                    return Err(cli_error(
                        ErrorCode::ExtraArgument,
                        "--help must be used alone",
                    ));
                }
                help_seen = true;
            }
            "--server" => {
                if help_seen {
                    return Err(cli_error(
                        ErrorCode::ExtraArgument,
                        "--help must be used alone",
                    ));
                }
                if server_seen {
                    return Err(cli_error(
                        ErrorCode::ExtraArgument,
                        "duplicate --server option",
                    ));
                }
                let value = args.next().ok_or_else(|| {
                    cli_error(ErrorCode::MissingArgument, "--server requires an address")
                })?;
                if value.is_empty() || value.starts_with('-') {
                    return Err(cli_error(
                        ErrorCode::MissingArgument,
                        "--server requires an address",
                    ));
                }
                config.server = value.parse().map_err(|_| {
                    cli_error(
                        ErrorCode::InvalidRequest,
                        format!("invalid server address: {value}"),
                    )
                })?;
                server_seen = true;
            }
            option if option.starts_with('-') => {
                return Err(cli_error(
                    ErrorCode::UnknownCommand,
                    format!("unknown option: {option}"),
                ));
            }
            argument => {
                return Err(cli_error(
                    ErrorCode::ExtraArgument,
                    format!("unexpected argument: {argument}"),
                ));
            }
        }
    }

    if help_seen {
        Ok(None)
    } else {
        Ok(Some(config))
    }
}

pub fn help_text() -> &'static str {
    "Usage: kv-client [--server HOST:PORT] [--help]\n\nCommands:\n  set KEY VALUE | get KEY | delete KEY | keys | status | storage-status | compact | ping | quit\n"
}

/// 连接服务端并进入交互循环。
pub async fn run(config: ClientConfig) -> Result<()> {
    let stream = TcpStream::connect(config.server).await?;
    let (reader, mut writer) = stream.into_split();
    let mut network_reader = BufReader::new(reader);
    let mut input = BufReader::new(io::stdin());
    let mut output = io::stdout();

    run_with_io(&mut input, &mut output, &mut network_reader, &mut writer).await
}

/// 把终端输入、输出和网络流分开，便于测试。
pub async fn run_with_io<I, O, R, W>(
    input: &mut I,
    output: &mut O,
    network_reader: &mut R,
    network_writer: &mut W,
) -> Result<()>
where
    I: AsyncBufRead + Unpin,
    O: AsyncWrite + Unpin,
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    output.write_all(COMMANDS.as_bytes()).await?;
    output.write_all(b"\n").await?;
    write_prompt(output).await?;

    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line).await? == 0 {
            return Ok(());
        }

        let request = match parse_command(&line) {
            Ok(request) => request,
            Err(error) => {
                print_error(output, &error).await?;
                write_prompt(output).await?;
                continue;
            }
        };
        let wants_quit = matches!(request, Request::Quit);

        let encoded = encode_request_line(&request)?;
        network_writer.write_all(&encoded).await?;
        network_writer.flush().await?;

        let response = receive_response(network_reader).await?;
        display_response(output, &response).await?;
        if wants_quit && matches!(response.data, Some(ResponseData::Quit)) {
            return Ok(());
        }
        write_prompt(output).await?;
    }
}

async fn receive_response<R>(reader: &mut R) -> Result<Response>
where
    R: AsyncBufRead + Unpin,
{
    match read_frame_async(reader).await? {
        Frame::Line(line) => parse_response_bytes(&line),
        Frame::TooLarge => Err(AppError::protocol(
            ErrorCode::FrameTooLarge,
            "server response is too large",
        )),
        Frame::Incomplete => Err(AppError::protocol(
            ErrorCode::InvalidRequest,
            "server response is incomplete",
        )),
        Frame::Eof => Err(AppError::protocol(
            ErrorCode::InvalidRequest,
            "server closed the connection before responding",
        )),
    }
}

async fn display_response<W>(output: &mut W, response: &Response) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let message = if response.ok {
        match response.data.as_ref() {
            Some(ResponseData::Set { replaced: true }) => "replaced\n".to_owned(),
            Some(ResponseData::Set { replaced: false }) => "created\n".to_owned(),
            Some(ResponseData::Get { value }) => format!("value: {value}\n"),
            Some(ResponseData::Delete { deleted }) => format!("deleted: {deleted}\n"),
            Some(ResponseData::Keys { keys, count }) => {
                format!("keys ({count}): {}\n", keys.join(", "))
            }
            Some(ResponseData::Status { count }) => format!("status: count={count}\n"),
            Some(ResponseData::StorageStatus {
                entries,
                wal_records,
                wal_bytes,
                snapshot_bytes,
                last_sequence,
                writable,
            }) => format!(
                "storage: entries={entries}, wal_records={wal_records}, wal_bytes={wal_bytes}, \
snapshot_bytes={snapshot_bytes}, last_sequence={last_sequence}, writable={writable}\n"
            ),
            Some(ResponseData::Compact {
                entries,
                compact_ms,
                wal_records_before,
                wal_bytes_before,
                snapshot_bytes_before,
                last_sequence_before,
                wal_records_after,
                wal_bytes_after,
                snapshot_bytes_after,
                last_sequence_after,
            }) => format!(
                "compacted in {compact_ms} ms: entries={entries}, \
WAL {wal_records_before} records/{wal_bytes_before} bytes -> \
{wal_records_after} records/{wal_bytes_after} bytes, \
snapshot {snapshot_bytes_before} -> {snapshot_bytes_after} bytes, \
sequence {last_sequence_before} -> {last_sequence_after}\n"
            ),
            Some(ResponseData::Ping) => "pong\n".to_owned(),
            Some(ResponseData::Quit) => "bye\n".to_owned(),
            None => {
                return Err(AppError::protocol(
                    ErrorCode::InvalidRequest,
                    "success response has no data",
                ));
            }
        }
    } else {
        let error = response.error.as_ref().ok_or_else(|| {
            AppError::protocol(ErrorCode::InvalidRequest, "failure response has no error")
        })?;
        format!("[{}] {}\n", error.code, error.message)
    };

    output.write_all(message.as_bytes()).await?;
    output.flush().await?;
    Ok(())
}

async fn print_error<W>(output: &mut W, error: &AppError) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let message = format!("[{}] {}\n", error.code(), error.client_message());
    output.write_all(message.as_bytes()).await?;
    output.flush().await
}

async fn write_prompt<W>(output: &mut W) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    output.write_all(PROMPT.as_bytes()).await?;
    output.flush().await
}

fn cli_error(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError::protocol(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_and_server_share_the_default_address() {
        assert_eq!(
            ClientConfig::default().server.to_string(),
            DEFAULT_BIND_ADDRESS
        );
    }

    #[test]
    fn command_line_address_is_checked() {
        let config = parse_client_args(["--server", "127.0.0.1:9000"])
            .unwrap()
            .unwrap();
        assert_eq!(config.server.to_string(), "127.0.0.1:9000");

        let invalid = parse_client_args(["--server", "bad-address"]).unwrap_err();
        assert_eq!(invalid.code(), ErrorCode::InvalidRequest);
    }
}
