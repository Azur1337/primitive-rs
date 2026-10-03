//! Progressive web demo for primitive-rs.
//!
//! Serves a single-page upload form plus a Server-Sent Events (SSE) stream that
//! replays the optimization one shape at a time as SVG, so the user watches the
//! image get drawn. This is an optional binary, gated behind the `web` feature:
//!
//! ```text
//! cargo run --features web --bin primitive-web
//! ```
//!
//! It reuses the `primitive` library directly (no shelling out to the CLI). The
//! CPU-bound `Model` loop runs on a blocking thread, off the async runtime, and
//! is serialized by a one-permit semaphore so a single run can use every core
//! without two uploads saturating the machine.

use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::{self, Stream};
use primitive::color::Color;
use primitive::model::Model;
use primitive::shape::ShapeType;
use primitive::util::{average_image_color, image_to_rgba, thumbnail};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{watch, Semaphore};

const INDEX_HTML: &str = include_str!("../web/index.html");

// Hard cap on jobs held in memory (active + queued + finished-but-not-reaped).
// Each queued job holds a ~256px target image and a channel, so this bounds the
// memory a burst of uploads can pin. Beyond this, new uploads are rejected with
// 503 so a flood can't grow the queue without bound.
const MAX_JOBS: usize = 8;

// Self-hosted Geist subsets (the same Latin woff2 the portfolio ships), baked
// into the binary so the deploy stays a single rebuilt image.
const GEIST_SANS: &[u8] = include_bytes!("../web/fonts/GeistSans.latin.woff2");
const GEIST_MONO: &[u8] = include_bytes!("../web/fonts/GeistMono.latin.woff2");

/// One SSE payload: the current SVG document and whether the run is finished.
#[derive(Clone)]
struct Frame {
    svg: String,
    done: bool,
}

impl Frame {
    /// Encode as a JSON object so embedded newlines/quotes in the SVG survive
    /// the single-line SSE `data:` field.
    fn to_json(&self) -> String {
        let mut map = serde_json::Map::new();
        map.insert(
            "svg".to_string(),
            serde_json::Value::String(self.svg.clone()),
        );
        map.insert("done".to_string(), serde_json::Value::Bool(self.done));
        serde_json::Value::Object(map).to_string()
    }
}

/// Shared server state.
struct AppState {
    /// Live jobs: a `watch` receiver holding the latest frame, keyed by id.
    jobs: Mutex<HashMap<u64, watch::Receiver<Frame>>>,
    next_id: AtomicU64,
    /// Only one run at a time: each run saturates every core via rayon.
    slots: Arc<Semaphore>,
}

impl AppState {
    fn new() -> Self {
        AppState {
            jobs: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            slots: Arc::new(Semaphore::new(1)),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Minimal current-thread runtime is fine: the heavy work runs on a blocking
    // thread; the async side only does I/O and channel waits.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let addr = format!(
        "{}:{}",
        std::env::var("ADDR").unwrap_or_else(|_| "0.0.0.0".to_string()),
        std::env::var("PORT").unwrap_or_else(|_| "6123".to_string()),
    );

    let state = Arc::new(AppState::new());
    {
        let s = Arc::clone(&state);
        tokio::spawn(async move { reaper(s).await });
    }

    let app = Router::new()
        .route("/", get(index))
        .route("/fonts/GeistSans.latin.woff2", get(font_sans))
        .route("/fonts/GeistMono.latin.woff2", get(font_mono))
        .route("/jobs", post(create_job))
        .route("/jobs/:id/events", get(events))
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("primitive-web listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// Serve an embedded woff2 with the right MIME type + long immutable cache.
fn font_response(bytes: &'static [u8]) -> Response {
    (
        [
            (axum::http::header::CONTENT_TYPE, "font/woff2".to_string()),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
        ],
        bytes.to_vec(),
    )
        .into_response()
}

async fn font_sans() -> Response {
    font_response(GEIST_SANS)
}

async fn font_mono() -> Response {
    font_response(GEIST_MONO)
}

/// Accept the upload + options, start a background run, and return its id.
async fn create_job(State(state): State<Arc<AppState>>, mut multipart: Multipart) -> Response {
    let mut file: Option<Vec<u8>> = None;
    let mut count = 100i32;
    let mut mode = 1i32;
    let mut alpha = 128i32;
    let mut size = 1024i32;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "file" => file = field.bytes().await.ok().map(|b| b.to_vec()),
            "count" => count = parse_i32(field.text().await.ok().as_deref()).unwrap_or(count),
            "mode" => mode = parse_i32(field.text().await.ok().as_deref()).unwrap_or(mode),
            "alpha" => alpha = parse_i32(field.text().await.ok().as_deref()).unwrap_or(alpha),
            "size" => size = parse_i32(field.text().await.ok().as_deref()).unwrap_or(size),
            _ => {}
        }
    }

    let bytes = match file {
        Some(b) if !b.is_empty() => b,
        _ => return (StatusCode::BAD_REQUEST, "missing file field").into_response(),
    };

    // Decode + downscale up front so a bad upload fails fast with a clear error.
    let decoded = match image::load_from_memory(&bytes) {
        Ok(im) => im,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("could not decode image: {e}"),
            )
                .into_response()
        }
    };
    let input = thumbnail(&decoded, 256);
    let bg = average_image_color(&input);
    let target = image_to_rgba(&input);

    // Clamp options to safe ranges.
    let count = count.clamp(1, 500);
    let size = size.clamp(64, 1024);
    let alpha = alpha.clamp(0, 255);
    let shape_type = shape_type_from_mode(mode);

    // Reject when the queue is full so a burst of uploads can't grow it without
    // bound. The reaper frees finished jobs within ~60s, so this only trips on
    // genuine load.
    {
        let jobs = match state.jobs.lock() {
            Ok(j) => j,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "state poisoned").into_response(),
        };
        if jobs.len() >= MAX_JOBS {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "server busy, try again shortly",
            )
                .into_response();
        }
    }

    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = watch::channel(Frame {
        svg: String::new(),
        done: false,
    });
    if let Ok(mut jobs) = state.jobs.lock() {
        jobs.insert(id, rx);
    }

    let slots = Arc::clone(&state.slots);
    tokio::spawn(async move {
        // Queue behind other runs: each one uses every core.
        let _permit = match slots.acquire().await {
            Ok(p) => p,
            Err(_) => {
                let _ = tx.send(Frame {
                    svg: String::new(),
                    done: true,
                });
                return;
            }
        };
        let _ = tokio::task::spawn_blocking(move || {
            run_model(target, bg, shape_type, alpha, count, size, tx)
        })
        .await;
    });

    Json(serde_json::json!({ "id": id })).into_response()
}

/// Stream a job's frames as SSE. Emits the current frame, then waits for each
/// change until the run signals `done` (or the sender is dropped).
async fn events(State(state): State<Arc<AppState>>, Path(id): Path<u64>) -> Response {
    let rx = match state.jobs.lock() {
        Ok(jobs) => match jobs.get(&id) {
            Some(rx) => rx.clone(),
            None => return (StatusCode::NOT_FOUND, "no such job").into_response(),
        },
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "state poisoned").into_response(),
    };

    let stream = frame_stream(rx);
    Sse::new(stream)
        .keep_alive(KeepAlive::default().interval(Duration::from_secs(5)))
        .into_response()
}

/// Build the SSE stream from a watch receiver using `unfold`.
fn frame_stream(rx: watch::Receiver<Frame>) -> impl Stream<Item = Result<Event, Infallible>> {
    // State machine carried by Option<Receiver>; None ends the stream.
    stream::unfold(
        Some(rx),
        |state: Option<watch::Receiver<Frame>>| async move {
            let mut rx = match state {
                Some(rx) => rx,
                None => return None,
            };
            let frame = rx.borrow_and_update().clone();
            let event = Event::default().event("frame").data(frame.to_json());
            if frame.done {
                Some((Ok(event), None))
            } else if rx.changed().await.is_err() {
                // Sender dropped without a final done frame; emit what we have, end.
                Some((Ok(event), None))
            } else {
                Some((Ok(event), Some(rx)))
            }
        },
    )
}

/// Periodically drop jobs whose run has finished (sender dropped).
async fn reaper(state: Arc<AppState>) {
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.retain(|_, rx| rx.clone().has_changed().is_ok());
        }
    }
}

/// Number of parallel workers for a run. Defaults to every available core
/// (fastest); set `PRIMITIVE_WORKERS` to a positive integer to cap it, leaving
/// headroom for other services on a shared box.
fn worker_count() -> i32 {
    let all = std::thread::available_parallelism()
        .map(|n| n.get() as i32)
        .unwrap_or(1);
    if let Ok(s) = std::env::var("PRIMITIVE_WORKERS") {
        if let Ok(n) = s.trim().parse::<i32>() {
            if n > 0 {
                return n.min(all);
            }
        }
    }
    all
}

/// The CPU-bound loop: build the model, add shapes one at a time, publishing
/// the SVG after each step.
fn run_model(
    target: image::RgbaImage,
    bg: Color,
    shape_type: ShapeType,
    alpha: i32,
    count: i32,
    size: i32,
    tx: watch::Sender<Frame>,
) {
    let workers = worker_count();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    let mut model = Model::new(&target, bg, size, workers, seed);
    let _ = tx.send(Frame {
        svg: model.svg(),
        done: false,
    });
    for _ in 0..count {
        model.step(shape_type, alpha, 0);
        // If every receiver is gone, stop early.
        if tx
            .send(Frame {
                svg: model.svg(),
                done: false,
            })
            .is_err()
        {
            return;
        }
    }
    let _ = tx.send(Frame {
        svg: model.svg(),
        done: true,
    });
}

fn shape_type_from_mode(mode: i32) -> ShapeType {
    match mode {
        0 => ShapeType::Any,
        1 => ShapeType::Triangle,
        2 => ShapeType::Rectangle,
        3 => ShapeType::Ellipse,
        4 => ShapeType::Circle,
        5 => ShapeType::RotatedRectangle,
        6 => ShapeType::Quadratic,
        7 => ShapeType::RotatedEllipse,
        8 => ShapeType::Polygon,
        _ => ShapeType::Triangle,
    }
}

fn parse_i32(s: Option<&str>) -> Option<i32> {
    s.and_then(|v| v.trim().parse::<i32>().ok())
}
