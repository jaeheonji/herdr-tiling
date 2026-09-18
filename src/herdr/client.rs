//! Synchronous client for the Herdr protocol 22 methods used by this plugin.

use std::{
    collections::HashSet,
    error, fmt,
    io::{self, BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tree::{Direction, Layout, Node, Ratio, Rect, SplitDirection};

/// A contextual socket, JSON, or protocol failure.
#[derive(Debug)]
pub enum Error {
    /// A local socket operation failed.
    Io {
        /// Operation being attempted.
        operation: &'static str,
        /// Underlying operating-system error.
        source: io::Error,
    },
    /// JSON encoding or decoding failed.
    Json {
        /// Operation being attempted.
        operation: &'static str,
        /// Underlying JSON error.
        source: serde_json::Error,
    },
    /// Herdr returned an error or violated the expected response contract.
    Protocol(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Json { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Protocol(message) => formatter.write_str(message),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::Protocol(_) => None,
        }
    }
}

/// Pane identity returned after a successful move.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MovedPane {
    /// Whether Herdr performed the requested move.
    pub changed: bool,
    /// Pane ID after the move.
    pub pane_id: String,
    /// Reason for a no-op move.
    pub reason: Option<String>,
    /// Destination tab ID.
    pub tab_id: String,
}

/// Blocking client for Herdr's newline-delimited Unix socket API.
#[derive(Debug)]
pub struct Client {
    socket_path: PathBuf,
    next_id: u64,
}

impl Client {
    /// Creates a client that opens a fresh socket for each request.
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_owned(),
            next_id: 1,
        }
    }

    /// Exports and validates the tab containing `pane_id` as a BSP tree.
    pub fn export_layout(&mut self, pane_id: &str) -> Result<Layout, Error> {
        self.export(ExportParams {
            pane_id: Some(pane_id),
            tab_id: None,
        })
    }

    /// Exports and validates `tab_id` as a BSP tree.
    pub fn export_tab_layout(&mut self, tab_id: &str) -> Result<Layout, Error> {
        self.export(ExportParams {
            pane_id: None,
            tab_id: Some(tab_id),
        })
    }

    fn export(&mut self, params: ExportParams<'_>) -> Result<Layout, Error> {
        let result = self.call("layout.export", &params, "layout_export")?;
        let result: LayoutResult = decode(result, "decode layout.export response")?;
        validate_layout(result.layout)
    }

    /// Reads the tab's outer area, including when its panes are zoomed.
    /// Rejects a tab change between layout export and this request.
    ///
    /// # Errors
    ///
    /// Returns a socket or protocol error, or rejects an empty, overflowing,
    /// or mismatched layout area.
    pub fn layout_area(&mut self, pane_id: &str, tab_id: &str) -> Result<Rect, Error> {
        let result = self.call("pane.layout", &PaneParams { pane_id }, "pane_layout")?;
        let result: PaneLayoutResult = decode(result, "decode pane.layout response")?;
        let area = result.layout.area;
        if result.layout.tab_id != tab_id {
            return Err(Error::Protocol(
                "pane.layout: pane changed tabs during export".into(),
            ));
        }
        if area.width == 0
            || area.height == 0
            || area.x.checked_add(area.width).is_none()
            || area.y.checked_add(area.height).is_none()
        {
            return Err(Error::Protocol(
                "pane.layout: invalid or empty layout area".into(),
            ));
        }
        Ok(area)
    }

    /// Finds the closest pane in `direction`, if one exists.
    pub fn neighbor(
        &mut self,
        pane_id: &str,
        direction: Direction,
    ) -> Result<Option<String>, Error> {
        let result = self.call(
            "pane.neighbor",
            &NeighborParams {
                pane_id,
                direction: direction_name(direction),
            },
            "pane_neighbor",
        )?;
        let result: NeighborResult = decode(result, "decode pane.neighbor response")?;
        Ok(result.neighbor.neighbor_pane_id)
    }

    /// Moves a pane beside `target_pane_id` in an existing tab.
    pub fn move_to_tab(
        &mut self,
        pane_id: &str,
        tab_id: &str,
        target_pane_id: &str,
        split: SplitDirection,
        ratio: Ratio,
    ) -> Result<MovedPane, Error> {
        self.move_pane(
            pane_id,
            MoveDestination::Tab {
                tab_id,
                split: split_name(split),
                ratio: ratio.get(),
                target_pane_id,
            },
        )
    }

    /// Moves a pane into a new scratch tab in the original workspace.
    pub fn move_to_new_tab(
        &mut self,
        pane_id: &str,
        workspace_id: &str,
    ) -> Result<MovedPane, Error> {
        self.move_pane(pane_id, MoveDestination::NewTab { workspace_id })
    }

    /// Restores one split ratio addressed by its root-relative child path.
    pub fn set_split_ratio(
        &mut self,
        tab_id: &str,
        path: &[bool],
        ratio: Ratio,
    ) -> Result<(), Error> {
        self.call(
            "layout.set_split_ratio",
            &SplitRatioParams {
                tab_id,
                path,
                ratio: ratio.get(),
            },
            "layout_split_ratio_set",
        )?;
        Ok(())
    }

    /// Focuses a pane by ID.
    pub fn focus(&mut self, pane_id: &str) -> Result<(), Error> {
        self.call("pane.focus", &PaneParams { pane_id }, "pane_info")?;
        Ok(())
    }

    /// Turns zoom on or off for a pane.
    pub fn zoom(&mut self, pane_id: &str, zoomed: bool) -> Result<(), Error> {
        self.call(
            "pane.zoom",
            &ZoomParams {
                pane_id,
                mode: if zoomed { "on" } else { "off" },
            },
            "pane_zoom",
        )?;
        Ok(())
    }

    fn move_pane(
        &mut self,
        pane_id: &str,
        destination: MoveDestination<'_>,
    ) -> Result<MovedPane, Error> {
        let result = self.call(
            "pane.move",
            &MoveParams {
                pane_id,
                destination,
                focus: false,
            },
            "pane_move",
        )?;
        let result: MoveResult = decode(result, "decode pane.move response")?;
        Ok(MovedPane {
            changed: result.move_result.changed,
            pane_id: result.move_result.pane.pane_id,
            reason: result.move_result.reason,
            tab_id: result.move_result.pane.tab_id,
        })
    }

    fn call(
        &mut self,
        method: &'static str,
        params: &impl Serialize,
        expected_type: &str,
    ) -> Result<Value, Error> {
        let id = format!("herdr-tiling-{}", self.next_id);
        self.next_id += 1;
        let request = serde_json::to_vec(&Request {
            id: &id,
            method,
            params,
        })
        .map_err(|source| Error::Json {
            operation: "encode Herdr request",
            source,
        })?;

        let mut stream = UnixStream::connect(&self.socket_path).map_err(|source| Error::Io {
            operation: "connect to Herdr socket",
            source,
        })?;
        stream.write_all(&request).map_err(|source| Error::Io {
            operation: "write Herdr request",
            source,
        })?;
        stream.write_all(b"\n").map_err(|source| Error::Io {
            operation: "write Herdr request delimiter",
            source,
        })?;

        let mut response = String::new();
        BufReader::new(stream)
            .read_line(&mut response)
            .map_err(|source| Error::Io {
                operation: "read Herdr response",
                source,
            })?;
        parse_response(&response, &id, method, expected_type)
    }
}

#[derive(Serialize)]
struct Request<'a, P> {
    id: &'a str,
    method: &'static str,
    params: P,
}

#[derive(Deserialize)]
struct Response {
    id: String,
    result: Option<Value>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    code: String,
    message: String,
}

#[derive(Serialize)]
struct ExportParams<'a> {
    pane_id: Option<&'a str>,
    tab_id: Option<&'a str>,
}

#[derive(Deserialize)]
struct LayoutResult {
    layout: ExportLayout,
}

#[derive(Deserialize)]
struct PaneLayoutResult {
    layout: PaneLayoutArea,
}

#[derive(Deserialize)]
struct PaneLayoutArea {
    tab_id: String,
    area: Rect,
}

#[derive(Deserialize)]
struct ExportLayout {
    workspace_id: String,
    tab_id: String,
    zoomed: bool,
    focused_pane_id: String,
    root: ExportNode,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ExportNode {
    Pane {
        pane_id: Option<String>,
    },
    Split {
        direction: ExportSplitDirection,
        ratio: f32,
        first: Box<Self>,
        second: Box<Self>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExportSplitDirection {
    Right,
    Down,
}

#[derive(Serialize)]
struct NeighborParams<'a> {
    pane_id: &'a str,
    direction: &'static str,
}

#[derive(Deserialize)]
struct NeighborResult {
    neighbor: Neighbor,
}

#[derive(Deserialize)]
struct Neighbor {
    neighbor_pane_id: Option<String>,
}

#[derive(Serialize)]
struct MoveParams<'a> {
    pane_id: &'a str,
    destination: MoveDestination<'a>,
    focus: bool,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MoveDestination<'a> {
    Tab {
        tab_id: &'a str,
        split: &'static str,
        ratio: f32,
        target_pane_id: &'a str,
    },
    NewTab {
        workspace_id: &'a str,
    },
}

#[derive(Deserialize)]
struct MoveResult {
    move_result: MoveResultBody,
}

#[derive(Deserialize)]
struct MoveResultBody {
    changed: bool,
    pane: MovedPaneBody,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct MovedPaneBody {
    pane_id: String,
    tab_id: String,
}

#[derive(Serialize)]
struct SplitRatioParams<'a> {
    tab_id: &'a str,
    path: &'a [bool],
    ratio: f32,
}

#[derive(Serialize)]
struct PaneParams<'a> {
    pane_id: &'a str,
}

#[derive(Serialize)]
struct ZoomParams<'a> {
    pane_id: &'a str,
    mode: &'static str,
}

fn parse_response(
    response: &str,
    request_id: &str,
    method: &str,
    expected_type: &str,
) -> Result<Value, Error> {
    if response.is_empty() {
        return Err(Error::Protocol(format!(
            "{method}: Herdr closed the socket without a response"
        )));
    }

    let response: Response = serde_json::from_str(response).map_err(|source| Error::Json {
        operation: "decode Herdr response",
        source,
    })?;
    if response.id != request_id {
        return Err(Error::Protocol(format!(
            "{method}: response ID {:?} does not match request ID {request_id:?}",
            response.id
        )));
    }
    if let Some(error) = response.error {
        return Err(Error::Protocol(format!(
            "{method}: Herdr error {}: {}",
            error.code, error.message
        )));
    }
    let result = response
        .result
        .ok_or_else(|| Error::Protocol(format!("{method}: response has no result or error")))?;
    let actual_type = result
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Protocol(format!("{method}: response result has no string type")))?;
    if actual_type != expected_type {
        return Err(Error::Protocol(format!(
            "{method}: expected result type {expected_type:?}, got {actual_type:?}"
        )));
    }
    Ok(result)
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value, operation: &'static str) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|source| Error::Json { operation, source })
}

fn validate_layout(export: ExportLayout) -> Result<Layout, Error> {
    let mut pane_ids = HashSet::new();
    let root = validate_node(export.root, &mut pane_ids)?;
    if !pane_ids.contains(&export.focused_pane_id) {
        return Err(Error::Protocol(format!(
            "layout.export: focused pane {:?} is not present in the tree",
            export.focused_pane_id
        )));
    }
    Ok(Layout {
        workspace_id: export.workspace_id,
        tab_id: export.tab_id,
        zoomed: export.zoomed,
        focused_pane_id: export.focused_pane_id,
        root,
    })
}

fn validate_node(export: ExportNode, pane_ids: &mut HashSet<String>) -> Result<Node, Error> {
    match export {
        ExportNode::Pane { pane_id } => {
            let id = pane_id.filter(|id| !id.is_empty()).ok_or_else(|| {
                Error::Protocol("layout.export: pane node has no pane ID".to_owned())
            })?;
            if !pane_ids.insert(id.clone()) {
                return Err(Error::Protocol(format!(
                    "layout.export: duplicate pane ID {id:?}"
                )));
            }
            Ok(Node::Pane { id })
        }
        ExportNode::Split {
            direction,
            ratio,
            first,
            second,
        } => Ok(Node::Split {
            direction: match direction {
                ExportSplitDirection::Right => SplitDirection::Right,
                ExportSplitDirection::Down => SplitDirection::Down,
            },
            ratio: Ratio::new(ratio)
                .map_err(|message| Error::Protocol(format!("layout.export: {message}")))?,
            first: Box::new(validate_node(*first, pane_ids)?),
            second: Box::new(validate_node(*second, pane_ids)?),
        }),
    }
}

fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Left => "left",
        Direction::Right => "right",
        Direction::Up => "up",
        Direction::Down => "down",
    }
}

fn split_name(direction: SplitDirection) -> &'static str {
    match direction {
        SplitDirection::Right => "right",
        SplitDirection::Down => "down",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::net::UnixListener,
        sync::atomic::{AtomicU64, Ordering},
        thread,
    };

    use super::*;

    const SUCCESS: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/layout_export_success.json"
    ));
    const MALFORMED: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/layout_export_malformed.json"
    ));
    const API_ERROR: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/error_response.json"
    ));

    #[test]
    fn exports_a_validated_layout_without_a_live_server() {
        let (socket, server) = serve_once(SUCCESS, |request| {
            assert_eq!(request["method"], "layout.export");
            assert_eq!(request["params"]["pane_id"], "w1:p1");
        });
        let layout = Client::new(socket).export_layout("w1:p1").unwrap();
        server.join().unwrap();

        assert_eq!(layout.workspace_id, "w1");
        assert_eq!(layout.tab_id, "w1:t1");
        assert_eq!(layout.focused_pane_id, "w1:p1");
        assert_eq!(
            layout.root,
            Node::Split {
                direction: SplitDirection::Right,
                ratio: Ratio::new(0.6).unwrap(),
                first: Box::new(Node::Pane { id: "w1:p1".into() }),
                second: Box::new(Node::Pane { id: "w1:p2".into() }),
            }
        );

        let (socket, server) = serve_once(SUCCESS, |request| {
            assert_eq!(request["method"], "layout.export");
            assert_eq!(request["params"]["tab_id"], "w1:t1");
            assert!(request["params"]["pane_id"].is_null());
        });
        assert_eq!(
            Client::new(socket).export_tab_layout("w1:t1").unwrap(),
            layout
        );
        server.join().unwrap();
    }

    #[test]
    fn rejects_malformed_and_error_responses() {
        for (response, message) in [
            (MALFORMED, "duplicate pane ID"),
            (API_ERROR, "tab_not_found"),
            ("not json\n", "decode Herdr response"),
        ] {
            let (socket, server) = serve_once(response, |_| {});
            let error = Client::new(socket)
                .export_layout("w1:p1")
                .unwrap_err()
                .to_string();
            server.join().unwrap();
            assert!(error.contains(message), "{error:?}");
        }
    }

    #[test]
    fn rejects_invalid_ratios_and_incomplete_splits() {
        let mut response: Value = serde_json::from_str(SUCCESS).unwrap();
        response["result"]["layout"]["root"]["ratio"] = Value::from(1.1);
        let result = parse_response(
            &response.to_string(),
            "herdr-tiling-1",
            "layout.export",
            "layout_export",
        )
        .and_then(|result| decode::<LayoutResult>(result, "decode layout.export response"))
        .and_then(|result| validate_layout(result.layout));
        assert!(result.unwrap_err().to_string().contains("between 0 and 1"));

        response["result"]["layout"]["root"]["ratio"] = Value::from(0.5);
        response["result"]["layout"]["root"]
            .as_object_mut()
            .unwrap()
            .remove("second");
        let result = parse_response(
            &response.to_string(),
            "herdr-tiling-1",
            "layout.export",
            "layout_export",
        )
        .and_then(|result| decode::<LayoutResult>(result, "decode layout.export response"));
        let Err(error) = result else {
            panic!("incomplete split was accepted");
        };
        assert!(error.to_string().contains("missing field"));
    }

    #[test]
    fn reads_layout_area_and_rejects_stale_or_empty_geometry() {
        const RESPONSE: &str = r#"{"id":"herdr-tiling-1","result":{"type":"pane_layout","layout":{"tab_id":"w1:t1","area":{"x":3,"y":7,"width":201,"height":81}}}}"#;
        let (socket, server) = serve_once(RESPONSE, |request| {
            assert_eq!(request["method"], "pane.layout");
            assert_eq!(request["params"]["pane_id"], "w1:p1");
        });
        assert_eq!(
            Client::new(socket).layout_area("w1:p1", "w1:t1").unwrap(),
            Rect {
                x: 3,
                y: 7,
                width: 201,
                height: 81
            }
        );
        server.join().unwrap();

        let (socket, server) = serve_once(RESPONSE, |_| {});
        assert!(
            Client::new(socket)
                .layout_area("w1:p1", "w1:t2")
                .unwrap_err()
                .to_string()
                .contains("changed tabs")
        );
        server.join().unwrap();

        let (socket, server) = serve_once(
            r#"{"id":"herdr-tiling-1","result":{"type":"pane_layout","layout":{"tab_id":"w1:t1","area":{"x":0,"y":0,"width":0,"height":40}}}}"#,
            |_| {},
        );
        assert!(
            Client::new(socket)
                .layout_area("w1:p1", "w1:t1")
                .unwrap_err()
                .to_string()
                .contains("empty layout area")
        );
        server.join().unwrap();
    }

    #[test]
    fn sends_the_required_mutation_and_neighbor_requests() {
        let (socket, server) = serve_once(
            "{\"id\":\"herdr-tiling-1\",\"result\":{\"type\":\"pane_neighbor\",\"neighbor\":{\"neighbor_pane_id\":\"w1:p2\"}}}",
            |request| {
                assert_eq!(request["method"], "pane.neighbor");
                assert_eq!(request["params"]["direction"], "left");
            },
        );
        assert_eq!(
            Client::new(socket)
                .neighbor("w1:p1", Direction::Left)
                .unwrap(),
            Some("w1:p2".into())
        );
        server.join().unwrap();

        let (socket, server) = serve_once(
            "{\"id\":\"herdr-tiling-1\",\"result\":{\"type\":\"pane_move\",\"move_result\":{\"changed\":true,\"reason\":null,\"pane\":{\"pane_id\":\"w1:p1\",\"tab_id\":\"w1:t2\"}}}}",
            |request| {
                assert_eq!(request["method"], "pane.move");
                assert_eq!(request["params"]["destination"]["type"], "tab");
                assert_eq!(request["params"]["destination"]["split"], "down");
            },
        );
        assert_eq!(
            Client::new(socket)
                .move_to_tab(
                    "w1:p1",
                    "w1:t2",
                    "w1:p2",
                    SplitDirection::Down,
                    Ratio::new(0.5).unwrap(),
                )
                .unwrap(),
            MovedPane {
                changed: true,
                pane_id: "w1:p1".into(),
                reason: None,
                tab_id: "w1:t2".into(),
            }
        );
        server.join().unwrap();

        let (socket, server) = serve_once(
            "{\"id\":\"herdr-tiling-1\",\"result\":{\"type\":\"layout_split_ratio_set\"}}",
            |request| {
                assert_eq!(request["method"], "layout.set_split_ratio");
                assert_eq!(request["params"]["path"], serde_json::json!([false, true]));
            },
        );
        Client::new(socket)
            .set_split_ratio("w1:t1", &[false, true], Ratio::new(0.6).unwrap())
            .unwrap();
        server.join().unwrap();

        let (socket, server) = serve_once(
            "{\"id\":\"herdr-tiling-1\",\"result\":{\"type\":\"pane_info\"}}",
            |request| assert_eq!(request["method"], "pane.focus"),
        );
        Client::new(socket).focus("w1:p1").unwrap();
        server.join().unwrap();

        let (socket, server) = serve_once(
            "{\"id\":\"herdr-tiling-1\",\"result\":{\"type\":\"pane_zoom\"}}",
            |request| {
                assert_eq!(request["method"], "pane.zoom");
                assert_eq!(request["params"]["mode"], "on");
            },
        );
        Client::new(socket).zoom("w1:p1", true).unwrap();
        server.join().unwrap();
    }

    fn serve_once(
        response: &'static str,
        inspect: impl FnOnce(Value) + Send + 'static,
    ) -> (PathBuf, thread::JoinHandle<()>) {
        static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);

        let directory = std::env::temp_dir().join(format!(
            "herdr-tiling-test-{}-{}",
            std::process::id(),
            NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let socket = directory.join("herdr.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            inspect(serde_json::from_str(&request).unwrap());
            stream.write_all(response.as_bytes()).unwrap();
            fs::remove_dir_all(directory).unwrap();
        });
        (socket, server)
    }
}
