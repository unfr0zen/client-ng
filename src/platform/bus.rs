//! The UI's place on the system bus: it answers nlheartbeatd, and holds
//! the unit awake through nlsleepd while the display is lit.
//!
//! nlheartbeatd (/etc/nestlabs/nlheartbeat.plist) calls
//! com.nestlabs.Heartbeat.Request on the owner of com.nestlabs.nlclient, at
//! /, every 5 seconds, and takes a reply of `true` as the ack; after 8
//! unacknowledged calls it restarts the UI, and watchdogd stops feeding the
//! hardware watchdog. The calls are answered
//! from the render loop, so an answer means the UI itself is alive.
//!
//! nlsleepd (com.nestlabs.scpm.sleepd) suspends the unit once every
//! registered entity has released it and agreed to RequestSleep. Like
//! nlclient, the UI registers as /com/nestlabs/nlClientApp, which holds the
//! unit until it calls ReleaseResource; it releases when the display goes
//! dark, retains when it lights, and declines RequestSleep while lit. After
//! a suspend nlsleepd calls WakeUp, and counts the UI as holding the unit
//! again until it releases. When nlsleepd restarts it forgets its
//! entities, so the UI registers again whenever the name gets a new owner.
//!
//! Where nlheartbeatd takes heartbeats unsolicited instead (the gen 2's
//! plist's Source), the UI calls com.nestlabs.Heartbeat.Request on
//! com.nestlabs.nlheartbeat itself, from the render loop, every half period.
//!
//! libdbus is the unit's own, loaded at run time like EGL.

use std::ffi::{c_char, c_int, c_void, CStr};

use libloading::Library;

const NAME: &CStr = c"com.nestlabs.nlclient";
const INTERFACE: &CStr = c"com.nestlabs.Heartbeat";
const METHOD: &CStr = c"Request";
const HEARTBEAT: &CStr = c"com.nestlabs.nlheartbeat";
const SLEEPD: &CStr = c"com.nestlabs.scpm.sleepd";
const SLEEPD_PATH: &CStr = c"/com/nestlabs/scpm/sleepd";
const MANAGER: &CStr = c"com.nestlabs.scpm.sleep_manager";
const CLIENT: &CStr = c"com.nestlabs.scpm.sleep_client";
const ENTITY: &CStr = c"/com/nestlabs/nlClientApp";
const OWNER_CHANGED: &CStr =
    c"type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0='com.nestlabs.scpm.sleepd'";

const DBUS_BUS_SYSTEM: c_int = 1;
const DBUS_NAME_FLAG_REPLACE_EXISTING: u32 = 2;
const DBUS_NAME_FLAG_DO_NOT_QUEUE: u32 = 4;
const DBUS_REQUEST_NAME_REPLY_PRIMARY_OWNER: c_int = 1;
const DBUS_TYPE_INVALID: c_int = 0;
const DBUS_TYPE_BOOLEAN: c_int = b'b' as c_int;
const DBUS_TYPE_STRING: c_int = b's' as c_int;

/// libdbus's DBusError: two strings and private fields, kept opaque.
#[repr(C)]
struct DBusError {
    name: *const c_char,
    message: *const c_char,
    _private: [usize; 4],
}

type Conn = *mut c_void;
type Message = *mut c_void;

/// What nlsleepd asked since the last tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sleep {
    /// The UI agreed to a suspend.
    pub sleeping: bool,
    /// The unit came back from a suspend.
    pub woke: bool,
}

pub struct Bus {
    _lib: Library,
    connection: Conn,
    /// Whether nlsleepd counts the UI as holding the unit awake.
    holding: bool,
    /// How often the UI sends nlheartbeatd its heartbeat, where it takes
    /// them unsolicited, and when it last did.
    beat: Option<(std::time::Duration, std::time::Instant)>,
    read_write: unsafe extern "C" fn(Conn, c_int) -> u32,
    pop_message: unsafe extern "C" fn(Conn) -> Message,
    is_method_call: unsafe extern "C" fn(Message, *const c_char, *const c_char) -> u32,
    is_signal: unsafe extern "C" fn(Message, *const c_char, *const c_char) -> u32,
    get_args: unsafe extern "C" fn(Message, *mut DBusError, c_int, ...) -> u32,
    error_init: unsafe extern "C" fn(*mut DBusError),
    error_free: unsafe extern "C" fn(*mut DBusError),
    new_method_return: unsafe extern "C" fn(Message) -> Message,
    new_method_call: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, *const c_char) -> Message,
    append_args: unsafe extern "C" fn(Message, c_int, ...) -> u32,
    send: unsafe extern "C" fn(Conn, Message, *mut u32) -> u32,
    flush: unsafe extern "C" fn(Conn),
    unref: unsafe extern "C" fn(Message),
}

fn sym<T: Copy>(lib: &Library, name: &[u8]) -> Result<T, String> {
    // SAFETY: each caller declares T as libdbus's signature of `name`.
    unsafe { lib.get::<T>(name) }.map(|s| *s)
        .map_err(|e| format!("{}: {e}", String::from_utf8_lossy(&name[..name.len() - 1])))
}

impl Bus {
    /// Connects to the system bus, takes over the UI's bus name and
    /// registers with nlsleepd, which then counts the UI as holding the unit;
    /// `unsolicited` is the heartbeat's period where the UI sends it.
    pub fn open(unsolicited: Option<std::time::Duration>) -> Result<Bus, String> {
        // SAFETY: libdbus runs no initialisers that depend on us.
        let lib = unsafe { Library::new("libdbus-1.so.3") }.map_err(|e| format!("libdbus-1.so.3: {e}"))?;
        let error_init: unsafe extern "C" fn(*mut DBusError) = sym(&lib, b"dbus_error_init\0")?;
        let error_free: unsafe extern "C" fn(*mut DBusError) = sym(&lib, b"dbus_error_free\0")?;
        let bus_get: unsafe extern "C" fn(c_int, *mut DBusError) -> Conn = sym(&lib, b"dbus_bus_get\0")?;
        let request_name: unsafe extern "C" fn(Conn, *const c_char, u32, *mut DBusError) -> c_int = sym(&lib, b"dbus_bus_request_name\0")?;
        let set_exit_on_disconnect: unsafe extern "C" fn(Conn, u32) = sym(&lib, b"dbus_connection_set_exit_on_disconnect\0")?;
        let add_match: unsafe extern "C" fn(Conn, *const c_char, *mut DBusError) = sym(&lib, b"dbus_bus_add_match\0")?;
        let bus = Bus {
            connection: std::ptr::null_mut(),
            holding: true,
            beat: unsolicited.map(|period| (period / 2, std::time::Instant::now() - period)),
            read_write: sym(&lib, b"dbus_connection_read_write\0")?,
            pop_message: sym(&lib, b"dbus_connection_pop_message\0")?,
            is_method_call: sym(&lib, b"dbus_message_is_method_call\0")?,
            is_signal: sym(&lib, b"dbus_message_is_signal\0")?,
            get_args: sym(&lib, b"dbus_message_get_args\0")?,
            error_init,
            error_free,
            new_method_return: sym(&lib, b"dbus_message_new_method_return\0")?,
            new_method_call: sym(&lib, b"dbus_message_new_method_call\0")?,
            append_args: sym(&lib, b"dbus_message_append_args\0")?,
            send: sym(&lib, b"dbus_connection_send\0")?,
            flush: sym(&lib, b"dbus_connection_flush\0")?,
            unref: sym(&lib, b"dbus_message_unref\0")?,
            _lib: lib,
        };
        // SAFETY: libdbus calls on an initialised error, freed after a failure.
        unsafe {
            let mut error: DBusError = std::mem::zeroed();
            error_init(&mut error);
            let failed = |error: &mut DBusError, what: &str| {
                let message = if error.message.is_null() { "failed".into() } else { CStr::from_ptr(error.message).to_string_lossy().into_owned() };
                error_free(error);
                format!("{what}: {message}")
            };
            let connection = bus_get(DBUS_BUS_SYSTEM, &mut error);
            if connection.is_null() {
                return Err(failed(&mut error, "system bus"));
            }
            set_exit_on_disconnect(connection, 0);
            let reply = request_name(connection, NAME.as_ptr(), DBUS_NAME_FLAG_REPLACE_EXISTING | DBUS_NAME_FLAG_DO_NOT_QUEUE, &mut error);
            if reply != DBUS_REQUEST_NAME_REPLY_PRIMARY_OWNER {
                return Err(if error.name.is_null() { format!("{} is owned by another process", NAME.to_string_lossy()) } else { failed(&mut error, "bus name") });
            }
            let bus = Bus { connection, ..bus };
            add_match(connection, OWNER_CHANGED.as_ptr(), &mut error);
            if !error.name.is_null() {
                warn!("{}; nlsleepd restarts go unnoticed", failed(&mut error, "bus match"));
            }
            bus.sleepd(c"RegisterEntity");
            info!("bus: {} owned, registered with nlsleepd{}", NAME.to_string_lossy(),
                  bus.beat.map_or(String::new(), |(every, _)| format!("; heartbeats sent every {} ms", every.as_millis())));
            Ok(bus)
        }
    }

    /// Calls a sleep_manager method that takes the entity's path, without
    /// waiting for its answer.
    fn sleepd(&self, method: &CStr) {
        debug!("sleep: {} to nlsleepd", method.to_string_lossy());
        // SAFETY: the connection is open; the message is unreferenced once sent.
        unsafe {
            let call = (self.new_method_call)(SLEEPD.as_ptr(), SLEEPD_PATH.as_ptr(), MANAGER.as_ptr(), method.as_ptr());
            if call.is_null() {
                warn!("sleep: {} not sent: no memory for the call", method.to_string_lossy());
                return;
            }
            let entity = ENTITY.as_ptr();
            if (self.append_args)(call, DBUS_TYPE_STRING, &entity as *const *const c_char, DBUS_TYPE_INVALID) != 0 {
                (self.send)(self.connection, call, std::ptr::null_mut());
            }
            (self.unref)(call);
            (self.flush)(self.connection);
        }
    }

    /// Holds the unit awake, or lets it sleep, through nlsleepd.
    pub fn hold(&mut self, awake: bool) {
        if awake != self.holding {
            self.sleepd(if awake { c"RetainResource" } else { c"ReleaseResource" });
            self.holding = awake;
        }
    }

    /// Whether a NameOwnerChanged signal gives nlsleepd's name a new owner.
    ///
    /// # Safety
    /// `message` is a live signal on the open connection.
    unsafe fn sleepd_started(&self, message: Message) -> bool {
        let mut error: DBusError = std::mem::zeroed();
        (self.error_init)(&mut error);
        let (mut name, mut old, mut new): (*const c_char, *const c_char, *const c_char) =
            (std::ptr::null(), std::ptr::null(), std::ptr::null());
        if (self.get_args)(message, &mut error, DBUS_TYPE_STRING, &mut name, DBUS_TYPE_STRING, &mut old,
                           DBUS_TYPE_STRING, &mut new, DBUS_TYPE_INVALID) == 0 {
            (self.error_free)(&mut error);
            return false;
        }
        CStr::from_ptr(name) == SLEEPD && !CStr::from_ptr(new).is_empty()
    }

    /// Answers the reply (entity, ok) nlsleepd's sleep_client calls expect.
    ///
    /// # Safety
    /// `message` is a live method call on the open connection.
    unsafe fn answer(&self, message: Message, ok: bool) {
        let reply = (self.new_method_return)(message);
        if reply.is_null() {
            return;
        }
        let (entity, ok) = (ENTITY.as_ptr(), u32::from(ok));
        if (self.append_args)(reply, DBUS_TYPE_STRING, &entity as *const *const c_char,
                              DBUS_TYPE_BOOLEAN, &ok as *const u32, DBUS_TYPE_INVALID) != 0 {
            (self.send)(self.connection, reply, std::ptr::null_mut());
        }
        (self.unref)(reply);
    }

    /// Answers the calls waiting: heartbeats, and nlsleepd's, agreeing to
    /// sleep only while `dark`. Everything else is dropped.
    pub fn tick(&mut self, dark: bool) -> Sleep {
        let mut sleep = Sleep::default();
        // SAFETY: the connection opened above; each popped message is unreferenced once.
        unsafe {
            if let Some((_, last)) = self.beat.as_mut().filter(|(every, last)| last.elapsed() >= *every) {
                *last = std::time::Instant::now();
                let call = (self.new_method_call)(HEARTBEAT.as_ptr(), c"/".as_ptr(), INTERFACE.as_ptr(), METHOD.as_ptr());
                if !call.is_null() {
                    trace!("bus: heartbeat sent");
                    (self.send)(self.connection, call, std::ptr::null_mut());
                    (self.unref)(call);
                }
            }
            (self.read_write)(self.connection, 0);
            loop {
                let message = (self.pop_message)(self.connection);
                if message.is_null() {
                    break;
                }
                if (self.is_method_call)(message, INTERFACE.as_ptr(), METHOD.as_ptr()) != 0 {
                    trace!("bus: heartbeat answered");
                    let reply = (self.new_method_return)(message);
                    let ack: u32 = 1;
                    if !reply.is_null() && (self.append_args)(reply, DBUS_TYPE_BOOLEAN, &ack as *const u32, DBUS_TYPE_INVALID) != 0 {
                        (self.send)(self.connection, reply, std::ptr::null_mut());
                        (self.unref)(reply);
                    }
                } else if (self.is_method_call)(message, CLIENT.as_ptr(), c"RequestSleep".as_ptr()) != 0 {
                    let ok = dark && !self.holding;
                    debug!("sleep: nlsleepd asks to sleep; {}", if ok { "agreed" } else if !dark { "declined, the display is lit" } else { "declined, holding the unit" });
                    sleep.sleeping |= ok;
                    self.answer(message, ok);
                } else if (self.is_signal)(message, c"org.freedesktop.DBus".as_ptr(), c"NameOwnerChanged".as_ptr()) != 0 {
                    if self.sleepd_started(message) {
                        info!("sleep: nlsleepd started; registering again");
                        self.sleepd(c"RegisterEntity");
                        self.holding = true;
                        if dark {
                            self.hold(false);
                        }
                    }
                } else if (self.is_method_call)(message, CLIENT.as_ptr(), c"WakeUp".as_ptr()) != 0 {
                    debug!("sleep: nlsleepd says the unit woke");
                    self.holding = true;
                    sleep.woke = true;
                    self.answer(message, true);
                }
                (self.unref)(message);
            }
            (self.flush)(self.connection);
        }
        sleep
    }
}

