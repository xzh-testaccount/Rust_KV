//! 异步客户端

use crate::error::{AppError, ErrorCode, Result};
use crate::protocol::{
    Frame, Response, ResponseData, encode_request_line, parse_command,
    read_frame_async, parse_response_line,
};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

const DEFAULT_SERVER: &str = "127.0.0.1:7878";
const PROMPT: &str = "kv> ";
const COMMANDS: &str =
    "commands: set KEY VALUE | get KEY | delete KEY | keys | status | ping | quit";

/// Command-line configuration for the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    pub server: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server: DEFAULT_SERVER.to_owned(),
        }
    }
}

impl ClientConfig {
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
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    let mut config = ClientConfig::default();
    let mut server_seen = false;
    let mut help_seen = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--help" => {
                if help_seen {
                    return Err(cli_error(
                        ErrorCode::ExtraArgument,
                        "--help may be specified only once",
                    ));
                }
                if server_seen {
                    return Err(cli_error(
                        ErrorCode::ExtraArgument,
                        "--help cannot be combined with other options",
                    ));
                }
                help_seen = true;
                index += 1;
            }
            "--server" => {
                if help_seen {
                    return Err(cli_error(
                        ErrorCode::ExtraArgument,
                        "--help cannot be combined with other options",
                    ));
                }
                if server_seen {
                    return Err(cli_error(
                        ErrorCode::ExtraArgument,
                        "--server may be specified only once",
                    ));
                }
                let Some(server) = args.get(index + 1) else {
                    return Err(cli_error(
                        ErrorCode::MissingArgument,
                        "--server requires an address",
                    ));
                };
                if server.starts_with('-') || server.is_empty() {
                    return Err(cli_error(
                        ErrorCode::MissingArgument,
                        "--server requires an address",
                    ));
                }
                config.server.clone_from(server);
                server_seen = true;
                index += 2;
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
    "Usage: kv-client [--server HOST:PORT] [--help]\n\nConnect to a key-value server.\n\nCommands: set KEY VALUE, get KEY, delete KEY, keys, status, ping, quit\n"
}

/// 辅助：构造客户端错误
fn client_error(message: impl Into<String>) -> AppError {
    AppError::Protocol {
        code: ErrorCode::InvalidRequest,
        message: message.into(),
    }
}

pub async fn run(config: ClientConfig) -> Result<()> {
    let mut stream = TcpStream::connect(&config.server)
        .await
        .map_err(|e| client_error(format!("connect failed: {e}")))?;
    let (reader, mut writer) = stream.split();

    let mut stdin = BufReader::new(io::stdin());
    let mut stdout = io::stdout();
    let mut net_reader = BufReader::new(reader);

    // 异步写入初始帮助信息和提示符
    stdout
        .write_all(COMMANDS.as_bytes())
        .await
        .map_err(AppError::Io)?;
    stdout.write_all(b"\n").await.map_err(AppError::Io)?;
    stdout
        .write_all(PROMPT.as_bytes())
        .await
        .map_err(AppError::Io)?;
    stdout.flush().await.map_err(AppError::Io)?;

    let mut line = String::new();
    loop {
        line.clear();

        let read = stdin
            .read_line(&mut line)
            .await
            .map_err(|e| client_error(format!("stdin read error: {e}")))?;
        if read == 0 {
            break;
        }

        let request = match parse_command(&line) {
            Ok(req) => req,
            Err(e) => {
                print_error(&mut stdout, &e).await?;
                write_prompt(&mut stdout).await?;
                continue;
            }
        };

        let quit = matches!(request, crate::protocol::Request::Quit);

        let encoded = match encode_request_line(&request) {
            Ok(data) => data,
            Err(e) => {
                print_error(&mut stdout, &e).await?;
                write_prompt(&mut stdout).await?;
                continue;
            }
        };

        if let Err(e) = writer.write_all(&encoded).await {
            let err = client_error(format!("send error: {e}"));
            print_error(&mut stdout, &err).await?;
            return Err(err);
        }

        let frame = match read_frame_async(&mut net_reader).await {
            Ok(frame) => frame,
            Err(e) => {
                let err = client_error(format!("receive error: {e}"));
                print_error(&mut stdout, &err).await?;
                return Err(err);
            }
        };

        let response_bytes = match frame {
            Frame::Line(bytes) => bytes,
            Frame::TooLarge => {
                let err = AppError::Protocol {
                    code: ErrorCode::FrameTooLarge,
                    message: "server response exceeded limit".to_owned(),
                };
                print_error(&mut stdout, &err).await?;
                return Err(err);
            }
            Frame::Incomplete => {
                let err = AppError::Protocol {
                    code: ErrorCode::InvalidRequest,
                    message: "incomplete response frame".to_owned(),
                };
                print_error(&mut stdout, &err).await?;
                return Err(err);
            }
            Frame::Eof => {
                return Ok(());
            }
        };

        let response_text = match std::str::from_utf8(&response_bytes) {
            Ok(t) => t,
            Err(_) => {
                let err = AppError::Protocol {
                    code: ErrorCode::InvalidUtf8,
                    message: "response not valid UTF-8".to_owned(),
                };
                print_error(&mut stdout, &err).await?;
                return Err(err);
            }
        };

        let response = match parse_response_line(response_text) {
            Ok(r) => r,
            Err(e) => {
                print_error(&mut stdout, &e).await?;
                return Err(e);
            }
        };

        if let Err(e) = display_response(&mut stdout, &response).await {
            print_error(&mut stdout, &e).await?;
            return Err(e);
        }

        if quit && matches!(response.data, Some(ResponseData::Quit)) {
            break;
        }

        write_prompt(&mut stdout).await?;
    }

    Ok(())
}

/// 显示响应（异步输出）
async fn display_response<W: tokio::io::AsyncWrite + std::marker::Unpin>(
    output: &mut W,
    response: &Response,
) -> Result<()> {
    if !response.ok {
        let error = response.error.as_ref().ok_or_else(|| AppError::Protocol {
            code: ErrorCode::InvalidRequest,
            message: "server returned error response without details".to_owned(),
        })?;
        let msg = format!("[{}] {}\n", error.code, error.message);
        output
            .write_all(msg.as_bytes())
            .await
            .map_err(AppError::Io)?;
        return Ok(());
    }

    let data = response.data.as_ref().ok_or_else(|| AppError::Protocol {
        code: ErrorCode::InvalidRequest,
        message: "server returned success response without data".to_owned(),
    })?;
    let msg = match data {
        ResponseData::Set { replaced } => {
            if *replaced {
                "replaced\n".to_owned()
            } else {
                "created\n".to_owned()
            }
        }
        ResponseData::Get { value } => format!("value: {}\n", value),
        ResponseData::Delete { deleted } => format!("deleted: {}\n", deleted),
        ResponseData::Keys { keys, count } => format!("keys ({}): {}\n", count, keys.join(", ")),
        ResponseData::Status { count } => format!("status: count={}\n", count),
        ResponseData::Ping => "pong\n".to_owned(),
        ResponseData::Quit => "bye\n".to_owned(),
    };
    output
        .write_all(msg.as_bytes())
        .await
        .map_err(AppError::Io)?;
    Ok(())
}

/// 打印错误（异步输出）
async fn print_error<W: tokio::io::AsyncWrite + std::marker::Unpin>(
    output: &mut W,
    error: &AppError,
) -> io::Result<()> {
    let msg = format!("[{}] {}\n", error.code(), error.client_message());
    output.write_all(msg.as_bytes()).await?;
    Ok(())
}

/// 打印提示符（异步输出）
async fn write_prompt<W: tokio::io::AsyncWrite + std::marker::Unpin>(
    output: &mut W,
) -> io::Result<()> {
    output.write_all(PROMPT.as_bytes()).await?;
    output.flush().await
}

fn cli_error(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError::Protocol {
        code,
        message: message.into(),
    }
}
