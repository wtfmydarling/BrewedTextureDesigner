//! The XDG desktop portal (D-Bus), where the desktop does what a Wayland program may not:
//! picking a color anywhere on the screen with the desktop's own color picker.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use futures::{FutureExt, StreamExt};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use super::Cancel;

const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";

#[derive(Debug)]
pub enum Error {
    /// This desktop has no such portal: pick another way.
    Unavailable(String),
    Failed(String),
}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Error {
        match &e {
            zbus::Error::MethodError(name, ..)
                if [
                    "org.freedesktop.DBus.Error.ServiceUnknown",
                    "org.freedesktop.DBus.Error.NameHasNoOwner",
                    "org.freedesktop.DBus.Error.UnknownMethod",
                    "org.freedesktop.DBus.Error.UnknownInterface",
                    "org.freedesktop.DBus.Error.UnknownObject",
                    "org.freedesktop.DBus.Error.NotSupported",
                    "org.freedesktop.portal.Error.NotAllowed",
                ]
                .contains(&name.as_str()) =>
            {
                Error::Unavailable(e.to_string())
            }
            // The portal is not installed: it could not even be started.
            zbus::Error::MethodError(name, ..) if name.as_str().starts_with("org.freedesktop.DBus.Error.Spawn.") => Error::Unavailable(e.to_string()),
            zbus::Error::InputOutput(_) | zbus::Error::Address(_) | zbus::Error::Handshake(_) => Error::Unavailable(e.to_string()),
            _ => Error::Failed(e.to_string()),
        }
    }
}

/// How a portal request ended.
struct Response {
    /// 0: done, 1: cancelled by the user, 2: ended another way.
    code: u32,
    results: HashMap<String, OwnedValue>,
}

/// Let the user pick a color with the desktop's color picker; `None` when they cancel.
pub fn pick_color(cancel: &Cancel) -> Result<Option<[f32; 4]>, Error> {
    let response = zbus::block_on(async {
        let conn = zbus::Connection::session().await.map_err(|e| Error::Unavailable(e.to_string()))?;
        request(&conn, "org.freedesktop.portal.Screenshot", "PickColor", |options| ("", options), cancel).await
    })?;
    match response {
        None => Ok(None),
        Some(Response { code: 0, results }) => {
            let color = results.get("color").and_then(|v| <(f64, f64, f64)>::try_from(Value::from(v.try_clone().ok()?)).ok());
            match color {
                Some((r, g, b)) if [r, g, b].iter().all(|c| c.is_finite()) => {
                    Ok(Some([r as f32, g as f32, b as f32].map(|c| c.clamp(0.0, 1.0))).map(|[r, g, b]| [r, g, b, 1.0]))
                }
                _ => Err(Error::Failed("The desktop's color picker gave no color".into())),
            }
        }
        Some(Response { code: 1, .. }) => Ok(None),
        // The desktop could not do it (a portal without a color picker, for one).
        Some(Response { .. }) => Err(Error::Unavailable("the desktop's color picker ended without a color".into())),
    }
}

/// The portals this desktop turned out not to have: asking again would only wait for
/// the same answer (D-Bus takes its time to tell).
fn known_missing(interface: &str) -> bool {
    static MISSING: Mutex<Vec<String>> = Mutex::new(Vec::new());
    super::lock(&MISSING).iter().any(|m| m == interface)
}

fn remember_missing(interface: &str) {
    static MISSING: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut missing = super::lock(&MISSING);
    if !missing.iter().any(|m| m == interface) {
        missing.push(interface.to_owned());
    }
}

/// Make a portal request and wait for its response; `None` if the app cancels it first.
/// `body` gets the options dictionary (with the request's token) and makes the arguments.
async fn request<B>(
    conn: &zbus::Connection,
    interface: &str,
    method: &str,
    body: impl FnOnce(HashMap<&'static str, Value<'static>>) -> B,
    cancel: &Cancel,
) -> Result<Option<Response>, Error>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    if known_missing(interface) {
        return Err(Error::Unavailable(format!("{interface} is not there")));
    }
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let token = format!("brewed_{}_{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    let sender = conn.unique_name().ok_or_else(|| Error::Unavailable("no name on the session bus".into()))?;
    let sender = sender.trim_start_matches(':').replace('.', "_");
    let expected = format!("{PATH}/request/{sender}/{token}");
    // Listen for the response before asking, so that it cannot be missed.
    let mut responses = response_stream(conn, &expected).await?;
    let options = HashMap::from([("handle_token", Value::from(token))]);
    let reply = conn.call_method(Some(DESTINATION), PATH, Some(interface), method, &body(options)).await;
    let reply = reply.map_err(|e| {
        let e = Error::from(e);
        if matches!(e, Error::Unavailable(_)) {
            remember_missing(interface);
        }
        e
    })?;
    let path: OwnedObjectPath = reply.body().deserialize()?;
    if path.as_str() != expected {
        // Portals older than 0.9 choose the path themselves.
        responses = response_stream(conn, path.as_str()).await?;
    }
    let (tx, rx) = futures::channel::oneshot::channel::<()>();
    let tx = std::sync::Mutex::new(Some(tx));
    cancel.on_cancel(move || {
        if let Some(tx) = super::lock(&tx).take() {
            let _ = tx.send(());
        }
    });
    futures::select! {
        message = responses.next().fuse() => {
            let message = message.ok_or_else(|| Error::Failed("the desktop portal went away".into()))??;
            let (code, results): (u32, HashMap<String, OwnedValue>) = message.body().deserialize()?;
            Ok(Some(Response { code, results }))
        }
        _ = rx.fuse() => {
            // Ends the desktop's interaction (it sends no response then).
            let _ = conn.call_method(Some(DESTINATION), path.as_str(), Some("org.freedesktop.portal.Request"), "Close", &()).await;
            Ok(None)
        }
    }
}

async fn response_stream(conn: &zbus::Connection, path: &str) -> Result<zbus::MessageStream, Error> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Request")?
        .member("Response")?
        .path(path.to_owned())?
        .build();
    Ok(zbus::MessageStream::for_match_rule(rule, conn, Some(4)).await?)
}

// ---------------------------------------------------------------- screen cast

/// A screen cast session: the desktop streams the monitors through PipeWire, live.
pub struct ScreenCast {
    /// Closing this ends the session, so it lives as long as the pick.
    connection: zbus::Connection,
    session: String,
    /// The socket of the PipeWire the streams are on.
    pub socket: std::os::fd::OwnedFd,
    pub streams: Vec<CastStream>,
}

/// One monitor of a screen cast session.
#[derive(Clone, Copy, Debug)]
pub struct CastStream {
    /// Its PipeWire node.
    pub node: u32,
    /// Where it is on the desktop, in logical units (the desktop may not say).
    pub position: Option<(i32, i32)>,
}

impl Drop for ScreenCast {
    fn drop(&mut self) {
        let (connection, session) = (self.connection.clone(), self.session.clone());
        zbus::block_on(async move { close_session(&connection, &session).await });
    }
}

/// Ask the desktop to stream the monitors; `None` if the user says no.
pub fn screen_cast(cancel: &Cancel) -> Result<Option<ScreenCast>, Error> {
    zbus::block_on(async {
        let conn = zbus::Connection::session().await.map_err(|e| Error::Unavailable(e.to_string()))?;
        let cast = "org.freedesktop.portal.ScreenCast";
        // The session the streams belong to.
        let created = request(
            &conn,
            cast,
            "CreateSession",
            |mut options| {
                options.insert("session_handle_token", Value::from(format!("brewed_{}", std::process::id())));
                (options,)
            },
            cancel,
        )
        .await?;
        let Some(created) = done(created)? else { return Ok(None) };
        let session: String = created
            .get("session_handle")
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .ok_or_else(|| Error::Failed("the desktop gave no screen cast session".into()))?;
        // Whatever happens next, the session does not stay open for nothing.
        let started = start_cast(&conn, cast, &session, cancel).await;
        match started {
            Ok(Some((socket, streams))) => Ok(Some(ScreenCast { connection: conn, session, socket, streams })),
            other => {
                close_session(&conn, &session).await;
                other.map(|_| None)
            }
        }
    })
}

/// Choose the monitors, start the cast and open its PipeWire socket.
async fn start_cast(conn: &zbus::Connection, cast: &str, session: &str, cancel: &Cancel) -> Result<Option<(std::os::fd::OwnedFd, Vec<CastStream>)>, Error> {
    let handle = || zbus::zvariant::ObjectPath::try_from(session.to_owned()).unwrap_or_default().to_owned();
    // Every monitor, without the pointer, remembered for the next time.
    let selected = request(
        conn,
        cast,
        "SelectSources",
        |mut options| {
            options.insert("types", Value::from(1u32));
            options.insert("multiple", Value::from(true));
            options.insert("cursor_mode", Value::from(1u32));
            options.insert("persist_mode", Value::from(2u32));
            if let Some(token) = remembered() {
                options.insert("restore_token", Value::from(token));
            }
            (handle(), options)
        },
        cancel,
    )
    .await?;
    if done(selected)?.is_none() {
        return Ok(None);
    }

    let started = request(conn, cast, "Start", |options| (handle(), "", options), cancel).await?;
    let Some(started) = done(started)? else { return Ok(None) };
    // The desktop will not ask again while this token lasts.
    if let Some(token) = started.get("restore_token").and_then(|v| String::try_from(v.try_clone().ok()?).ok()) {
        remember(&token);
    }
    let streams: Vec<CastStream> = started
        .get("streams")
        .and_then(|v| Vec::<(u32, HashMap<String, OwnedValue>)>::try_from(v.try_clone().ok()?).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|(node, properties)| {
            let pair = |key: &str| <(i32, i32)>::try_from(Value::from(properties.get(key)?.try_clone().ok()?)).ok();
            CastStream { node, position: pair("position") }
        })
        .collect();
    if streams.is_empty() {
        return Err(Error::Failed("the desktop shared no monitor".into()));
    }

    let reply = conn.call_method(Some(DESTINATION), PATH, Some(cast), "OpenPipeWireRemote", &(handle(), HashMap::<&str, Value<'_>>::new())).await?;
    let socket: zbus::zvariant::OwnedFd = reply.body().deserialize()?;
    Ok(Some((socket.into(), streams)))
}

async fn close_session(conn: &zbus::Connection, session: &str) {
    let _ = conn.call_method(Some(DESTINATION), session, Some("org.freedesktop.portal.Session"), "Close", &()).await;
}

/// The results of a request that went through; `None` if the user cancelled it.
fn done(response: Option<Response>) -> Result<Option<HashMap<String, OwnedValue>>, Error> {
    match response {
        None | Some(Response { code: 1, .. }) => Ok(None),
        Some(Response { code: 0, results }) => Ok(Some(results)),
        Some(Response { .. }) => Err(Error::Unavailable("the desktop ended the request".into())),
    }
}

/// Where the desktop's permission to stream the screen is kept, so that it is asked for
/// once and not at every pick.
fn token_path() -> Option<std::path::PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config")))?;
    Some(config.join("brewed-texture-designer").join("screen-cast.token"))
}

fn remembered() -> Option<String> {
    let token = std::fs::read_to_string(token_path()?).ok()?;
    let token = token.trim().to_owned();
    (!token.is_empty()).then_some(token)
}

fn remember(token: &str) {
    if let Some(path) = token_path()
        && let Some(folder) = path.parent()
        && std::fs::create_dir_all(folder).is_ok()
    {
        let _ = std::fs::write(path, token);
    }
}
