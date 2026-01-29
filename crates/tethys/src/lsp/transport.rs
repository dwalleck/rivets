//! JSON-RPC transport for LSP communication.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use lsp_types::{
    request::{GotoDefinition, Initialize, References, Shutdown},
    ClientCapabilities, GotoDefinitionParams, GotoDefinitionResponse, InitializeParams,
    InitializeResult, Location, PartialResultParams, Position, ReferenceContext, ReferenceParams,
    TextDocumentIdentifier, TextDocumentPositionParams, Uri, WorkDoneProgressParams,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use tracing::{debug, trace, warn};

use super::error::LspError;
use super::provider::LspProvider;
use super::Result;

/// LSP client for communicating with language servers.
///
/// Provides a thin JSON-RPC transport layer over stdin/stdout.
pub struct LspClient {
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    request_id: i64,
}

impl LspClient {
    /// Start an LSP server and perform the initialize handshake.
    ///
    /// # Arguments
    ///
    /// * `provider` - Configuration for which LSP server to start
    /// * `workspace_path` - Root directory of the workspace to analyze
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The LSP server executable is not found
    /// - The server fails to start
    /// - The initialize handshake fails
    ///
    /// # Panics
    ///
    /// Panics if stdin/stdout are not available after spawning the process.
    /// This should never happen when `Stdio::piped()` is used.
    #[must_use = "LSP client holds a running process that should be shut down"]
    pub fn start(provider: &dyn LspProvider, workspace_path: &Path) -> Result<Self> {
        let command = provider.command();
        let args = provider.args();

        debug!(
            command = command,
            args = ?args,
            workspace = %workspace_path.display(),
            "Starting LSP server"
        );

        // Spawn the LSP server process
        let mut process = Command::new(command)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    LspError::not_found(command, provider.install_hint())
                } else {
                    LspError::spawn_failed(command, e)
                }
            })?;

        let stdin = process.stdin.take().expect("stdin was piped");
        let stdout = process.stdout.take().expect("stdout was piped");
        let stdout = BufReader::new(stdout);

        let mut client = Self {
            process,
            stdin,
            stdout,
            request_id: 0,
        };

        // Perform initialize handshake
        client.initialize(workspace_path, provider.initialize_options())?;

        Ok(client)
    }

    /// Perform the LSP initialize handshake.
    #[allow(deprecated)] // root_uri is deprecated but still widely used
    fn initialize(&mut self, workspace_path: &Path, init_options: Option<Value>) -> Result<()> {
        let workspace_uri = path_to_uri(workspace_path)?;

        let params = InitializeParams {
            root_uri: Some(workspace_uri),
            capabilities: ClientCapabilities::default(),
            initialization_options: init_options,
            ..Default::default()
        };

        let _result: InitializeResult = self.send_request::<Initialize>(params)?;

        // Send initialized notification
        self.send_notification("initialized", &json!({}))?;

        debug!("LSP initialize handshake complete");
        Ok(())
    }

    /// Send an LSP request and wait for the response.
    ///
    /// Uses the `lsp_types::request::Request` trait to determine the method name
    /// and deserialize the response to the correct type.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails to serialize
    /// - Writing to the LSP server fails
    /// - Reading the response fails
    /// - The response contains an error
    /// - The response ID doesn't match the request ID
    pub fn send_request<R>(&mut self, params: R::Params) -> Result<R::Result>
    where
        R: lsp_types::request::Request,
        R::Params: Serialize,
        R::Result: DeserializeOwned,
    {
        self.request_id += 1;
        let id = self.request_id;

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": R::METHOD,
            "params": params,
        });

        trace!(method = R::METHOD, id = id, "Sending LSP request");

        self.write_message(&request)?;
        self.read_response(id)
    }

    /// Send an LSP notification (no response expected).
    fn send_notification(&mut self, method: &str, params: &Value) -> Result<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        trace!(method = method, "Sending LSP notification");
        self.write_message(&notification)
    }

    /// Write a JSON-RPC message to the server.
    fn write_message(&mut self, message: &Value) -> Result<()> {
        let body = serde_json::to_string(message).map_err(LspError::Serialize)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        trace!(body_len = body.len(), "Writing LSP message");

        self.stdin.write_all(header.as_bytes())?;
        self.stdin.write_all(body.as_bytes())?;
        self.stdin.flush()?;

        Ok(())
    }

    /// Read a JSON-RPC response from the server.
    fn read_response<T: DeserializeOwned>(&mut self, expected_id: i64) -> Result<T> {
        // Read Content-Length header
        let content_length = self.read_content_length()?;

        // Read the JSON body
        let mut body = vec![0u8; content_length];
        self.stdout.read_exact(&mut body)?;

        let response: Value = serde_json::from_slice(&body).map_err(LspError::Deserialize)?;

        trace!(response = %response, "Received LSP response");

        // Check for error response
        if let Some(error) = response.get("error") {
            let code = error["code"].as_i64().unwrap_or(-1);
            let message = error["message"].as_str().unwrap_or("unknown error");
            return Err(LspError::server_error(code, message));
        }

        // Verify response ID matches
        let actual_id = response["id"]
            .as_i64()
            .ok_or_else(|| LspError::InvalidHeader("response missing 'id' field".to_string()))?;

        if actual_id != expected_id {
            return Err(LspError::IdMismatch {
                expected: expected_id,
                actual: actual_id,
            });
        }

        // Deserialize the result
        let result = response.get("result").ok_or_else(|| {
            LspError::InvalidHeader("response missing 'result' field".to_string())
        })?;

        serde_json::from_value(result.clone()).map_err(LspError::Deserialize)
    }

    /// Read the Content-Length header from the response.
    fn read_content_length(&mut self) -> Result<usize> {
        let mut headers = String::new();

        loop {
            let mut line = String::new();
            let bytes_read = self.stdout.read_line(&mut line)?;

            if bytes_read == 0 {
                return Err(LspError::ServerExited);
            }

            // Empty line signals end of headers
            if line == "\r\n" || line == "\n" {
                break;
            }

            headers.push_str(&line);
        }

        // Parse Content-Length
        for line in headers.lines() {
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                return value.trim().parse().map_err(|_| {
                    LspError::InvalidHeader(format!("invalid Content-Length: {value}"))
                });
            }
        }

        Err(LspError::InvalidHeader(
            "missing Content-Length header".to_string(),
        ))
    }

    /// Get the definition location for a symbol at the given position.
    ///
    /// # Arguments
    ///
    /// * `file` - Path to the source file
    /// * `line` - Line number (0-indexed)
    /// * `col` - Column number (0-indexed)
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(Location))` if a definition is found, `Ok(None)` if no
    /// definition exists at that position.
    ///
    /// # Errors
    ///
    /// Returns an error if the file path is invalid or communication fails.
    pub fn goto_definition(
        &mut self,
        file: &Path,
        line: u32,
        col: u32,
    ) -> Result<Option<Location>> {
        let uri = path_to_uri(file)?;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(line, col),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response: Option<GotoDefinitionResponse> =
            self.send_request::<GotoDefinition>(params)?;

        Ok(response.and_then(Self::extract_first_location))
    }

    /// Find all references to a symbol at the given position.
    ///
    /// # Arguments
    ///
    /// * `file` - Path to the source file
    /// * `line` - Line number (0-indexed)
    /// * `col` - Column number (0-indexed)
    ///
    /// # Returns
    ///
    /// Returns a list of locations where the symbol is referenced.
    /// The declaration site itself is excluded from the results.
    ///
    /// # Errors
    ///
    /// Returns an error if the file path is invalid or communication fails.
    pub fn find_references(&mut self, file: &Path, line: u32, col: u32) -> Result<Vec<Location>> {
        let uri = path_to_uri(file)?;

        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(uri),
                position: Position::new(line, col),
            },
            context: ReferenceContext {
                include_declaration: false,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response: Option<Vec<Location>> = self.send_request::<References>(params)?;

        Ok(response.unwrap_or_default())
    }

    /// Gracefully shut down the LSP server.
    ///
    /// Sends a shutdown request followed by an exit notification.
    ///
    /// # Errors
    ///
    /// Returns an error if communication with the server fails.
    pub fn shutdown(&mut self) -> Result<()> {
        debug!("Shutting down LSP server");

        // Send shutdown request
        let _: () = self.send_request::<Shutdown>(())?;

        // Send exit notification
        self.send_notification("exit", &json!(null))?;

        // Wait for process to exit and verify clean shutdown
        match self.process.wait() {
            Ok(status) => {
                if !status.success() {
                    warn!(
                        exit_code = ?status.code(),
                        "LSP server exited with non-zero status"
                    );
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to wait for LSP server process exit");
            }
        }

        Ok(())
    }

    /// Extract the first Location from a `GotoDefinitionResponse`.
    fn extract_first_location(response: GotoDefinitionResponse) -> Option<Location> {
        match response {
            GotoDefinitionResponse::Scalar(loc) => Some(loc),
            GotoDefinitionResponse::Array(locs) => locs.into_iter().next(),
            GotoDefinitionResponse::Link(links) => links.into_iter().next().map(|link| Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            }),
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // Attempt graceful shutdown, but don't panic on failure
        if let Err(e) = self.send_notification("exit", &json!(null)) {
            warn!(error = %e, "Failed to send exit notification to LSP server during cleanup");
        }

        if let Err(e) = self.process.kill() {
            // InvalidInput means process already exited - not an error
            if e.kind() != std::io::ErrorKind::InvalidInput {
                warn!(error = %e, "Failed to kill LSP server process during cleanup");
            }
        }

        // Reap the process to prevent zombies
        let _ = self.process.wait();
    }
}

/// Convert a filesystem path to an LSP URI.
///
/// Creates a `file://` URI from the given path. On Unix, this produces URIs like
/// `file:///home/user/project/src/main.rs`. On Windows, it handles drive letters
/// appropriately.
fn path_to_uri(path: &Path) -> Result<Uri> {
    // Canonicalize the path to get an absolute path
    let absolute_path = path.canonicalize().map_err(|e| {
        LspError::InvalidPath(format!(
            "cannot canonicalize path '{}': {e}",
            path.display()
        ))
    })?;

    // Convert to string, handling platform differences
    let path_str = absolute_path.to_str().ok_or_else(|| {
        LspError::InvalidPath(format!("path contains invalid UTF-8: {}", path.display()))
    })?;

    // Build the file URI
    // On Unix: /home/user/file.rs -> file:///home/user/file.rs
    // On Windows: C:\Users\file.rs -> file:///C:/Users/file.rs
    #[cfg(windows)]
    let uri_string = format!("file:///{}", path_str.replace('\\', "/"));

    #[cfg(not(windows))]
    let uri_string = format!("file://{path_str}");

    uri_string
        .parse()
        .map_err(|e| LspError::InvalidPath(format!("invalid URI '{uri_string}': {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper to create a mock JSON-RPC response.
    fn mock_response(id: i64, result: &Value) -> String {
        let response = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        let body = serde_json::to_string(&response).unwrap();
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
    }

    /// Test helper to create a mock JSON-RPC error response.
    #[allow(dead_code)]
    fn mock_error_response(id: i64, code: i64, message: &str) -> String {
        let response = json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message,
            },
        });
        let body = serde_json::to_string(&response).unwrap();
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
    }

    #[test]
    fn mock_response_format_is_valid() {
        let response = mock_response(1, &json!({"capabilities": {}}));
        assert!(response.starts_with("Content-Length: "));
        assert!(response.contains("\r\n\r\n"));
        assert!(response.contains("\"id\":1"));
    }

    #[test]
    fn mock_error_response_format_is_valid() {
        let response = mock_error_response(2, -32600, "Invalid Request");
        assert!(response.starts_with("Content-Length: "));
        assert!(response.contains("\"error\""));
        assert!(response.contains("-32600"));
    }

    fn parse_uri(s: &str) -> Uri {
        s.parse().expect("valid URI")
    }

    #[test]
    fn extract_first_location_from_scalar() {
        let location = Location {
            uri: parse_uri("file:///test.rs"),
            range: lsp_types::Range::default(),
        };
        let response = GotoDefinitionResponse::Scalar(location.clone());

        let extracted = LspClient::extract_first_location(response);
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap().uri.as_str(), location.uri.as_str());
    }

    #[test]
    fn extract_first_location_from_array() {
        let location = Location {
            uri: parse_uri("file:///test.rs"),
            range: lsp_types::Range::default(),
        };
        let response = GotoDefinitionResponse::Array(vec![location.clone()]);

        let extracted = LspClient::extract_first_location(response);
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap().uri.as_str(), location.uri.as_str());
    }

    #[test]
    fn extract_first_location_from_empty_array_returns_none() {
        let response = GotoDefinitionResponse::Array(vec![]);

        let extracted = LspClient::extract_first_location(response);
        assert!(extracted.is_none());
    }

    #[test]
    fn extract_first_location_from_link() {
        use lsp_types::{LocationLink, Range};

        let link = LocationLink {
            origin_selection_range: None,
            target_uri: parse_uri("file:///target.rs"),
            target_range: Range::default(),
            target_selection_range: Range::default(),
        };
        let response = GotoDefinitionResponse::Link(vec![link.clone()]);

        let extracted = LspClient::extract_first_location(response);
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap().uri.as_str(), link.target_uri.as_str());
    }

    #[test]
    fn path_to_uri_creates_valid_file_uri() {
        // Test with a path that exists (current directory)
        let path = std::env::current_dir().expect("current dir exists");
        let uri = path_to_uri(&path).expect("path_to_uri should succeed");

        let uri_str = uri.as_str();
        assert!(
            uri_str.starts_with("file://"),
            "URI should start with file://"
        );
        assert!(
            !uri_str.contains('\\'),
            "URI should not contain backslashes"
        );
    }
}
