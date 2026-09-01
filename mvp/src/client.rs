//! Command-line client for the persistent key-value store.

use crate::error::{AppError, ErrorCode, Result};
use crate::protocol::{
    Frame, Request, Response, ResponseData, encode_request_line, parse_command,
    parse_response_line, read_frame,
};
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;

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
    /// Parse arguments including and after the executable name.
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

/// Parse arguments after the executable name. `Ok(None)` means help was requested.
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

/// Short command-line help text.
pub fn help_text() -> &'static str {
    "Usage: kv-client [--server HOST:PORT] [--help]\n\nConnect to a key-value server.\n\nCommands: set KEY VALUE, get KEY, delete KEY, keys, status, ping, quit\n"
}

/// Run the network client using process stdin/stdout and a parsed configuration.
pub fn run(config: ClientConfig) -> Result<()> {
    let stream = TcpStream::connect(&config.server)?;
    stream.set_nodelay(true)?;
    let reader_stream = stream.try_clone()?;
    let mut writer_stream = stream;
    let mut reader = BufReader::new(reader_stream);

    run_with_io(
        io::stdin().lock(),
        io::stdout().lock(),
        |request| {
            writer_stream.write_all(request)?;
            writer_stream.flush()?;
            Ok(())
        },
        || match read_frame(&mut reader)? {
            Frame::Line(bytes) => Ok(bytes),
            Frame::TooLarge => Err(AppError::protocol(
                ErrorCode::FrameTooLarge,
                "server response exceeded the frame limit",
            )),
            Frame::Incomplete => Err(AppError::protocol(
                ErrorCode::InvalidRequest,
                "server closed with an incomplete response frame",
            )),
            Frame::Eof => Err(AppError::Message(
                "server closed the connection before responding".to_owned(),
            )),
        },
    )
}

/// Run the interactive loop with injectable input/output and transport.
///
/// The receive callback must return one complete JSON Lines frame, including its LF.
/// Network callers should use [`read_frame`] to produce that frame.
pub fn run_with_io<R, W, Send, Receive>(
    mut input: R,
    mut output: W,
    mut send: Send,
    mut receive: Receive,
) -> Result<()>
where
    R: BufRead,
    W: Write,
    Send: FnMut(&[u8]) -> Result<()>,
    Receive: FnMut() -> Result<Vec<u8>>,
{
    writeln!(output, "{COMMANDS}")?;
    write_prompt(&mut output)?;

    let mut line = String::new();
    loop {
        line.clear();
        let read = input.read_line(&mut line)?;
        if read == 0 {
            return Ok(());
        }

        let request = match parse_command(&line) {
            Ok(request) => request,
            Err(error) => {
                print_error(&mut output, &error)?;
                write_prompt(&mut output)?;
                continue;
            }
        };

        let quit = matches!(request, Request::Quit);
        let encoded = match encode_request_line(&request) {
            Ok(encoded) => encoded,
            Err(error) => {
                print_error(&mut output, &error)?;
                write_prompt(&mut output)?;
                continue;
            }
        };

        if let Err(error) = send(&encoded) {
            print_error(&mut output, &error)?;
            return Err(error);
        }

        let response_bytes = match receive() {
            Ok(bytes) => bytes,
            Err(error) => {
                print_error(&mut output, &error)?;
                return Err(error);
            }
        };
        let response_text = match std::str::from_utf8(&response_bytes) {
            Ok(text) => text,
            Err(_) => {
                let error = AppError::protocol(
                    ErrorCode::InvalidUtf8,
                    "server response is not valid UTF-8",
                );
                print_error(&mut output, &error)?;
                return Err(error);
            }
        };
        let response = match parse_response_line(response_text) {
            Ok(response) => response,
            Err(error) => {
                print_error(&mut output, &error)?;
                return Err(error);
            }
        };

        if let Err(error) = display_response(&mut output, &response) {
            print_error(&mut output, &error)?;
            return Err(error);
        }
        if quit && matches!(response.data, Some(ResponseData::Quit)) {
            return Ok(());
        }
        write_prompt(&mut output)?;
    }
}

fn display_response<W: Write>(output: &mut W, response: &Response) -> Result<()> {
    if !response.ok {
        let error = response.error.as_ref().ok_or_else(|| {
            AppError::protocol(
                ErrorCode::InvalidRequest,
                "server returned an error response without error details",
            )
        })?;
        writeln!(output, "[{}] {}", error.code, error.message)?;
        return Ok(());
    }

    let data = response.data.as_ref().ok_or_else(|| {
        AppError::protocol(
            ErrorCode::InvalidRequest,
            "server returned a success response without data",
        )
    })?;
    match data {
        ResponseData::Set { replaced } => {
            writeln!(output, "{}", if *replaced { "replaced" } else { "created" })?
        }
        ResponseData::Get { value } => writeln!(output, "value: {value}")?,
        ResponseData::Delete { deleted } => writeln!(output, "deleted: {deleted}")?,
        ResponseData::Keys { keys, count } => {
            writeln!(output, "keys ({count}): {}", keys.join(", "))?
        }
        ResponseData::Status { count } => writeln!(output, "status: count={count}")?,
        ResponseData::Ping => writeln!(output, "pong")?,
        ResponseData::Quit => writeln!(output, "bye")?,
    }
    Ok(())
}

fn print_error<W: Write>(output: &mut W, error: &AppError) -> io::Result<()> {
    writeln!(output, "[{}] {}", error.code(), error.client_message())
}

fn write_prompt<W: Write>(output: &mut W) -> io::Result<()> {
    write!(output, "{PROMPT}")?;
    output.flush()
}

fn cli_error(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError::protocol(code, message)
}
