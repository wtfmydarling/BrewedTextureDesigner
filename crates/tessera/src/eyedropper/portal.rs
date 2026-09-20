//! The XDG desktop portal (D-Bus), where the desktop does what a Wayland program may not:
//! picking a color anywhere on the screen with the desktop's own color picker.

use std::collections::HashMap;
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
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let token = format!("tessera_{}_{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
    let sender = conn.unique_name().ok_or_else(|| Error::Unavailable("no name on the session bus".into()))?;
    let sender = sender.trim_start_matches(':').replace('.', "_");
    let expected = format!("{PATH}/request/{sender}/{token}");
    // Listen for the response before asking, so that it cannot be missed.
    let mut responses = response_stream(conn, &expected).await?;
    let options = HashMap::from([("handle_token", Value::from(token))]);
    let reply = conn.call_method(Some(DESTINATION), PATH, Some(interface), method, &body(options)).await?;
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
