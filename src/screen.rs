// SPDX-License-Identifier: GPL-3.0-only

//! Keeping the screen awake and bright while a barcode is on it.
//!
//! The presenter is the one screen in this application with a hard external
//! requirement: a gate reader either resolves the display or it does not, and
//! a laptop that dims after thirty seconds in a queue has failed at the only
//! job a wallet has. So while a barcode is up, two pieces of desktop state
//! are borrowed and — this is the part that has to be right — given back.
//!
//! **Idle inhibition** goes through the XDG portal
//! (`org.freedesktop.portal.Inhibit`) rather than a Wayland protocol or a
//! compositor-specific bus name. The portal is already a dependency of the
//! toolkit, it is the interface that keeps working inside a Flatpak, and it
//! is served on every desktop rather than only this one.
//!
//! **Brightness** goes through `com.system76.CosmicSettingsDaemon`, which
//! owns the backlight on this desktop and is the same path the brightness
//! keys take. The proxy is declared here rather than pulled from the bindings
//! crate because only three members of that interface are wanted and one of
//! them — `MaxDisplayBrightness` — the bindings do not carry.
//!
//! Both are best-effort by design. A missing portal or a settings daemon that
//! is not running makes the presenter dimmer, not broken, so every failure
//! here is logged and stepped over rather than shown to the user in front of
//! a queue.

use std::fmt;
use std::sync::Arc;

use ashpd::desktop::Request;
use ashpd::desktop::inhibit::{InhibitFlags, InhibitProxy};

#[zbus::proxy(
    interface = "com.system76.CosmicSettingsDaemon",
    default_service = "com.system76.CosmicSettingsDaemon",
    default_path = "/com/system76/CosmicSettingsDaemon"
)]
trait CosmicSettingsDaemon {
    /// `DisplayBrightness` property, in the backlight's own units.
    #[zbus(property)]
    fn display_brightness(&self) -> zbus::Result<i32>;
    fn set_display_brightness(&self, value: i32) -> zbus::Result<()>;

    /// The top of that range, which varies by device and is not a percentage.
    #[zbus(property)]
    fn max_display_brightness(&self) -> zbus::Result<i32>;
}

/// What the presenter borrowed, and therefore what it owes back.
///
/// Carried through the message loop, which is why it is `Clone` and `Debug`;
/// the portal request underneath is neither, so it travels behind an `Arc`
/// and its `Debug` says only that it exists.
#[derive(Clone, Default)]
pub struct Hold {
    /// Dropping this does *not* end the inhibition — the portal keeps it
    /// until the request is closed or the connection goes away — so
    /// [`release`] is not optional.
    idle: Option<Arc<Request<()>>>,
    /// The brightness to put back, if it was raised.
    restore: Option<i32>,
}

impl fmt::Debug for Hold {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Hold")
            .field("idle_inhibited", &self.idle.is_some())
            .field("restore_brightness", &self.restore)
            .finish()
    }
}

impl Hold {
    /// The brightness owed back, for the shutdown path that cannot await.
    #[must_use]
    pub const fn owed_brightness(&self) -> Option<i32> {
        self.restore
    }
}

/// Inhibits idling and raises the backlight, returning what to give back.
pub async fn acquire(reason: String) -> Hold {
    Hold {
        idle: inhibit_idle(&reason).await.map(Arc::new),
        restore: raise_brightness().await,
    }
}

/// Gives back everything [`acquire`] borrowed.
pub async fn release(hold: Hold) {
    if let Some(request) = hold.idle
        && let Err(why) = request.close().await
    {
        tracing::warn!(%why, "the idle inhibition could not be closed");
    }

    if let Some(previous) = hold.restore
        && let Err(why) = set_brightness(previous).await
    {
        tracing::warn!(%why, "the display brightness could not be restored");
    }
}

/// Puts the brightness back without an executor, for application shutdown.
///
/// The asynchronous path in [`release`] is the one that normally runs. This
/// exists because a window closed while a pass is being presented would
/// otherwise leave the backlight at maximum, which is a state the user did
/// not ask for and cannot connect to anything they did.
pub fn restore_blocking(brightness: i32) {
    // On its own thread: the caller is inside the toolkit's async runtime,
    // and the blocking bus API refuses to be driven from there.
    let restored = std::thread::spawn(move || -> zbus::Result<()> {
        let connection = zbus::blocking::Connection::session()?;
        CosmicSettingsDaemonProxyBlocking::new(&connection)?.set_display_brightness(brightness)
    })
    .join();

    match restored {
        Ok(Ok(())) => {}
        Ok(Err(why)) => tracing::warn!(%why, "the display brightness could not be restored"),
        Err(_) => tracing::warn!("the brightness-restoring thread panicked"),
    }
}

async fn inhibit_idle(reason: &str) -> Option<Request<()>> {
    let proxy = match InhibitProxy::new().await {
        Ok(proxy) => proxy,
        Err(why) => {
            tracing::warn!(%why, "no inhibit portal; the screen may blank while presenting");
            return None;
        }
    };

    // No window identifier: the portal accepts an empty parent, and obtaining
    // a real one means exporting the surface through xdg-foreign for a
    // request that shows no dialogue.
    match proxy.inhibit(None, InhibitFlags::Idle.into(), reason).await {
        Ok(request) => Some(request),
        Err(why) => {
            tracing::warn!(%why, "the idle inhibition was refused");
            None
        }
    }
}

/// Raises the backlight to its maximum, returning the level to put back.
///
/// `None` when nothing was changed — no daemon, no backlight, or a screen
/// already at maximum — which is also the answer that means "owe nothing".
async fn raise_brightness() -> Option<i32> {
    let connection = zbus::Connection::session().await.ok()?;
    let proxy = CosmicSettingsDaemonProxy::new(&connection).await.ok()?;

    let current = proxy.display_brightness().await.ok()?;
    let maximum = proxy.max_display_brightness().await.ok()?;
    // The daemon answers -1 when it has no backlight to speak for, which is
    // an external monitor or a desktop machine.
    if current < 0 || maximum <= 0 || current >= maximum {
        return None;
    }

    match proxy.set_display_brightness(maximum).await {
        Ok(()) => Some(current),
        Err(why) => {
            tracing::warn!(%why, "the display brightness could not be raised");
            None
        }
    }
}

async fn set_brightness(value: i32) -> zbus::Result<()> {
    let connection = zbus::Connection::session().await?;
    CosmicSettingsDaemonProxy::new(&connection)
        .await?
        .set_display_brightness(value)
        .await
}
