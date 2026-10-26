//! Skins: Lua that draws the UI's view of the thermostat, and the menus file.
//!
//! A skin is a folder whose `skin.lua` returns
//!
//! ```lua
//! return {
//!   name = "Classic", api = 1, design = 480,
//!   fonts = { regular = { "unit:AkkuratNest-Regular.ttf", "fonts/Inter-Regular.ttf" }, ... },
//!   draw = function(gfx, ui) ... return animating end,
//! }
//! ```
//!
//! A skin not animating can ask for its next frame later with
//! `gfx:wake(seconds)`.
//!
//! Lua runs sandboxed: the table, string, math and utf8 libraries, `log`,
//! and a `require` that only loads `.lua` files of the skin, or of the skins
//! beside it as `require("other:module")`. A skin reads
//! `ui` and draws through `gfx`; it cannot change settings or touch files.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{Function, HookTriggers, Lua, LuaOptions, StdLib, Table, UserData, UserDataMethods, Value as LuaValue, VmState};

use crate::menu::{Choice, Field, Item, Kind, Off, Shown, Tree};
use crate::model::Value;
use crate::paint::{Align, Baseline, Color, Painter, TextStyle};

/// The skin API this engine provides.
pub const API: i64 = 1;

/// How long a skin may run at once: loading, or drawing one frame. Past
/// it the skin fails and the engine draws without it, so a looping skin
/// cannot stop the UI (and with it the heartbeat the watchdog needs).
const TIME_LIMIT: Duration = Duration::from_millis(250);

/// When the running Lua must stop, if it is running.
type Deadline = Rc<Cell<Option<Instant>>>;

/// A Lua state with only what skins and menus may use.
fn sandbox(dir: &Path) -> mlua::Result<(Lua, Deadline)> {
    let lua = Lua::new_with(StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::BIT, LuaOptions::default())?;
    let deadline: Deadline = Rc::new(Cell::new(None));
    let watched = deadline.clone();
    lua.set_hook(HookTriggers::new().every_nth_instruction(1_000_000), move |_, _| {
        match watched.get() {
            Some(end) if Instant::now() > end => Err(mlua::Error::runtime(format!("ran for longer than {TIME_LIMIT:?}"))),
            _ => Ok(VmState::Continue),
        }
    })?;
    let globals = lua.globals();
    // The base library is always loaded: two of it read files, and loading
    // or dumping chunks would let bytecode, which is not checked, in.
    for name in ["dofile", "loadfile", "load", "loadstring"] {
        globals.set(name, LuaValue::Nil)?;
    }
    globals.get::<Table>("string")?.set("dump", LuaValue::Nil)?;
    // Case for every language, not only ASCII ("KÜHLEN", not "KüHLEN"); a
    // text that is not UTF-8 changes as the C library would.
    for (name, unicode) in [("upper", true), ("lower", false)] {
        let f = lua.create_function(move |lua, text: mlua::LuaString| {
            let bytes = text.as_bytes();
            match std::str::from_utf8(&bytes) {
                Ok(t) => lua.create_string(if unicode { t.to_uppercase() } else { t.to_lowercase() }),
                Err(_) => lua.create_string(if unicode { bytes.to_ascii_uppercase() } else { bytes.to_ascii_lowercase() }),
            }
        })?;
        globals.get::<Table>("string")?.set(name, f)?;
    }
    let log = lua.create_function(|_, args: mlua::Variadic<LuaValue>| {
        let parts: Vec<String> = args.iter().map(|v| v.to_string().unwrap_or_else(|_| format!("{v:?}"))).collect();
        debug!("skin: {}", parts.join(" "));
        Ok(())
    })?;
    globals.set("log", log.clone())?;
    // A text in the language in use (the client's i18n), for a skin's own words.
    globals.set("tr", lua.create_function(|_, text: String| Ok(crate::i18n::tr(&text)))?)?;
    // Seconds on a monotonic clock, for skins to time their own drawing.
    let epoch = Instant::now();
    globals.set("clock", lua.create_function(move |_, ()| Ok(epoch.elapsed().as_secs_f64()))?)?;
    globals.set("print", log)?;
    // Skins sit side by side in one folder; a module of another skin runs
    // with that skin's own modules.
    let skins: Rc<Path> = dir.parent().unwrap_or(Path::new(".")).into();
    let own = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let require = requirer(&lua, skins, lua.create_table()?, own.clone(), own)?;
    globals.set("require", require)?;
    Ok((lua, deadline))
}

/// Whether `name` can name a skin: its folder's name.
fn skin_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `require` for the modules of skin `skin`: `require("name")` loads its
/// `name.lua` (dots for subfolders), `require("other:name")` a module of the
/// skin `other` beside it. Each module is loaded once; another skin's
/// modules see a `require` of their own skin's.
fn requirer(lua: &Lua, skins: Rc<Path>, loaded: Table, main: String, skin: String) -> mlua::Result<Function> {
    lua.create_function(move |lua, name: String| {
        let (from, module) = name.split_once(':').unwrap_or((skin.as_str(), name.as_str()));
        let key = format!("{from}:{module}");
        let found: LuaValue = loaded.get(key.as_str())?;
        if !found.is_nil() {
            return Ok(found);
        }
        if !skin_name(from) || !module.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')) {
            return Err(mlua::Error::runtime(format!("require: bad module name {name:?}")));
        }
        let path = skins.join(from).join(format!("{}.lua", module.replace('.', "/")));
        let source = std::fs::read_to_string(&path)
            .map_err(|e| mlua::Error::runtime(format!("require {name}: {e}")))?;
        let mut chunk = lua.load(source);
        if from == main {
            chunk = chunk.set_name(format!("@{module}.lua"));
        } else {
            let env = lua.create_table()?;
            let meta = lua.create_table()?;
            meta.set("__index", lua.globals())?;
            env.set_metatable(Some(meta))?;
            env.set("require", requirer(lua, skins.clone(), loaded.clone(), main.clone(), from.to_string())?)?;
            chunk = chunk.set_name(format!("@{key}.lua")).set_environment(env);
        }
        let module: LuaValue = chunk.eval()?;
        let module = if module.is_nil() { LuaValue::Boolean(true) } else { module };
        loaded.set(key.as_str(), module.clone())?;
        Ok(module)
    })
}

/// Runs `f` with the Lua time limit armed.
fn limited<T>(deadline: &Deadline, f: impl FnOnce() -> T) -> T {
    deadline.set(Some(Instant::now() + TIME_LIMIT));
    let result = f();
    deadline.set(None);
    result
}

/// A loaded skin.
pub struct Skin {
    lua: Lua,
    deadline: Deadline,
    pub name: String,
    pub dir: PathBuf,
    /// The size of the face in the skin's own units.
    pub design: f32,
    /// Font names and their files.
    pub fonts: Vec<(String, PathBuf)>,
    draw: Function,
    /// Time spent turning the engine's view into Lua tables, until taken.
    pub converting: std::cell::Cell<Duration>,
    /// The soonest the last frame asked to be drawn again (`gfx:wake`), in
    /// seconds from it.
    wake: Cell<Option<f64>>,
    /// The last view given to Lua and its table: while only the time moves,
    /// the table is kept and its `now` updated.
    last: std::cell::RefCell<Option<(Ui, Table)>>,
}

impl std::fmt::Debug for Skin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Skin").field("name", &self.name).field("dir", &self.dir).finish_non_exhaustive()
    }
}

impl Skin {
    pub fn load(dir: &Path) -> Result<Skin, String> {
        let (lua, deadline) = sandbox(dir).map_err(|e| e.to_string())?;
        let entry = dir.join("skin.lua");
        let source = std::fs::read_to_string(&entry).map_err(|e| format!("{}: {e}", entry.display()))?;
        let table: Table = limited(&deadline, || lua.load(source).set_name("@skin.lua").eval()).map_err(|e| e.to_string())?;
        let api: i64 = table.get("api").map_err(|e| format!("skin.lua api: {e}"))?;
        if api != API {
            return Err(format!("skin targets API {api}; this engine provides {API}"));
        }
        let name: String = table.get("name").map_err(|e| format!("skin.lua name: {e}"))?;
        let design: f32 = table.get::<Option<f32>>("design").map_err(|e| e.to_string())?.unwrap_or(480.0);
        let draw: Function = table.get("draw").map_err(|e| format!("skin.lua draw: {e}"))?;
        let mut fonts = Vec::new();
        if let Some(list) = table.get::<Option<Table>>("fonts").map_err(|e| e.to_string())? {
            for pair in list.pairs::<String, LuaValue>() {
                let (font, files) = pair.map_err(|e| format!("skin.lua fonts: {e}"))?;
                let files: Vec<String> = match files {
                    LuaValue::String(s) => vec![s.to_str().map_err(|e| e.to_string())?.to_string()],
                    LuaValue::Table(t) => t.sequence_values::<String>().collect::<Result<_, _>>()
                        .map_err(|e| format!("skin.lua fonts.{font}: {e}"))?,
                    _ => return Err(format!("skin.lua fonts.{font}: a file or a list of files")),
                };
                fonts.push((font.clone(), font_file(dir, &files).map_err(|e| format!("skin.lua fonts.{font}: {e}"))?));
            }
        }
        fonts.sort();
        debug!("skin: {name} loaded from {}: design {design}, {} fonts", crate::log::path(dir), fonts.len());
        Ok(Skin { lua, deadline, name, dir: dir.to_path_buf(), design, fonts, draw, converting: Default::default(), wake: Cell::new(None), last: Default::default() })
    }

    /// Draws one frame; true while the skin is animating and wants another.
    pub fn draw(&self, painter: &mut dyn Painter, ui: &Ui) -> Result<bool, String> {
        let started = Instant::now();
        let mut last = self.last.borrow_mut();
        let state = match last.as_ref() {
            Some((prev, table)) if prev.same_but_now(ui) => {
                table.set("now", ui.now).map_err(|e| e.to_string())?;
                table.clone()
            }
            _ => {
                let table = ui.to_lua(&self.lua).map_err(|e| e.to_string())?;
                *last = Some((ui.clone(), table.clone()));
                table
            }
        };
        drop(last);
        self.converting.set(self.converting.get() + started.elapsed());
        self.wake.set(None);
        limited(&self.deadline, || self.lua.scope(|scope| {
            let gfx = scope.create_userdata(Gfx { painter, dir: &self.dir, wake: &self.wake })?;
            let result: LuaValue = self.draw.call((gfx, state))?;
            Ok(matches!(result, LuaValue::Boolean(true)))
        })).map_err(|e| e.to_string())
    }

    /// How long after the last frame the skin asked to be drawn again,
    /// when it did.
    pub fn wake(&self) -> Option<Duration> {
        self.wake.get().map(|s| Duration::from_secs_f64(s.max(0.0)))
    }
}

/// The first of a font's candidate files that exists: a file in the skin,
/// `other:FILE` in the skin `other` beside it, or `unit:NAME` in the unit's
/// fonts (`platform::share`), which stay on the unit: they are not ours to
/// ship. The last is taken as it stands, so a missing font is reported by
/// name.
fn font_file(dir: &Path, files: &[String]) -> Result<PathBuf, String> {
    let resolve = |file: &str| match file.split_once(':') {
        Some(("unit", name)) if !name.contains('/') && !name.starts_with('.') => Ok(crate::platform::share().join("fonts").join(name)),
        Some(("unit", _)) => Err(format!("{file:?} is not a font name")),
        Some((skin, path)) if skin_name(skin) => inside(&dir.parent().unwrap_or(Path::new(".")).join(skin), path),
        Some(_) => Err(format!("{file:?} names no skin")),
        None => inside(dir, file),
    };
    let paths = files.iter().map(|f| resolve(f)).collect::<Result<Vec<_>, _>>()?;
    paths.iter().find(|p| p.is_file()).or(paths.last()).cloned().ok_or_else(|| "no files".into())
}

/// A skin's text in the language in use, in capitals when it was.
fn shown(text: &str) -> String {
    let t = crate::i18n::tr(text);
    if t != text && text.chars().any(char::is_alphabetic) && text == text.to_uppercase() { t.to_uppercase() } else { t }
}

/// Where `run:` image names point: pictures unfr0zen's services make as
/// they run (Matter's pairing code).
const RUN_IMAGES: &str = "/var/run/unfr0zen";

/// An image a skin names: a file inside the skin, `unit:NAME` among the
/// unit's images, `run:NAME` among unfr0zen's, or `plugin:NAME` among the
/// plugins' files.
fn image_file(dir: &Path, file: &str) -> Result<String, String> {
    let named = |name: &str, root: &str| if !name.contains('/') && !name.starts_with('.') {
        Ok(format!("{root}/{name}"))
    } else {
        Err(format!("{file:?} is not an image name"))
    };
    if let Some(name) = file.strip_prefix("unit:") {
        named(name, &crate::platform::share().join("images").display().to_string())
    } else if let Some(name) = file.strip_prefix("run:") {
        named(name, RUN_IMAGES)
    } else if let Some(name) = file.strip_prefix("plugin:") {
        // The last folder holding it: a dropped-in plugin's picture beside it.
        let dirs = PLUGIN_DIRS.with(|d| d.borrow().clone());
        let dir = dirs.iter().rev().find(|d| d.join(name).is_file()).or(dirs.first())
            .map(|d| d.display().to_string()).unwrap_or_else(|| PLUGINS.into());
        named(name, &dir)
    } else {
        inside(dir, file).map(|_| file.to_string())
    }
}

/// A file a skin names, which must stay inside the skin's folder.
fn inside(dir: &Path, file: &str) -> Result<PathBuf, String> {
    let path = Path::new(file);
    if path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!("{file:?} is outside the skin"));
    }
    Ok(dir.join(path))
}

/// A fingerprint of the skin's files and the menus file (names, sizes and
/// times), to reload them when either changes.
pub fn stamp(paths: &[&Path]) -> u64 {
    use std::hash::{Hash, Hasher};
    fn walk(path: &Path, hasher: &mut std::collections::hash_map::DefaultHasher) {
        let Ok(meta) = std::fs::metadata(path) else { return };
        path.hash(hasher);
        meta.len().hash(hasher);
        meta.modified().ok().hash(hasher);
        if meta.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(path).into_iter().flatten().flatten().map(|e| e.path()).collect();
            entries.sort();
            for entry in entries {
                walk(&entry, hasher);
            }
        }
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for path in paths {
        walk(path, &mut hasher);
    }
    hasher.finish()
}

/// The drawing API a skin's `draw` receives as `gfx`. Angles are degrees,
/// clockwise from 12 o'clock.
struct Gfx<'a> {
    painter: &'a mut dyn Painter,
    dir: &'a Path,
    wake: &'a Cell<Option<f64>>,
}

fn color(value: Option<String>) -> mlua::Result<Color> {
    match value {
        Some(text) => Color::parse(&text).map_err(mlua::Error::runtime),
        None => Ok(Color::WHITE),
    }
}

fn style(table: Option<Table>) -> mlua::Result<TextStyle> {
    let mut style = TextStyle {
        font: "regular".into(),
        size: 24.0,
        color: Color::WHITE,
        align: Align::Left,
        baseline: Baseline::Alphabetic,
        spacing: 0.0,
        outline: None,
        snap: true,
    };
    let Some(t) = table else { return Ok(style) };
    if let Some(c) = t.get::<Option<String>>("outline")? {
        style.outline = Some((Color::parse(&c).map_err(mlua::Error::runtime)?, t.get::<Option<f32>>("outline_width")?.unwrap_or(2.0)));
    }
    if let Some(font) = t.get::<Option<String>>("font")? { style.font = font; }
    if let Some(size) = t.get::<Option<f32>>("size")? { style.size = size; }
    style.color = color(t.get("color")?)?;
    if let Some(spacing) = t.get::<Option<f32>>("spacing")? { style.spacing = spacing; }
    if let Some(snap) = t.get::<Option<bool>>("snap")? { style.snap = snap; }
    style.align = match t.get::<Option<String>>("align")?.as_deref() {
        None | Some("left") => Align::Left,
        Some("center") => Align::Center,
        Some("right") => Align::Right,
        Some(other) => return Err(mlua::Error::runtime(format!("align {other:?}"))),
    };
    style.baseline = match t.get::<Option<String>>("baseline")?.as_deref() {
        None | Some("alphabetic") => Baseline::Alphabetic,
        Some("top") => Baseline::Top,
        Some("middle") => Baseline::Middle,
        Some("bottom") => Baseline::Bottom,
        Some(other) => return Err(mlua::Error::runtime(format!("baseline {other:?}"))),
    };
    Ok(style)
}

impl UserData for Gfx<'_> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("circle", |_, g, (x, y, r, c): (f32, f32, f32, Option<String>)| {
            g.painter.fill_circle(x, y, r, color(c)?);
            Ok(())
        });
        methods.add_method_mut("ring", |_, g, (x, y, r, width, c): (f32, f32, f32, f32, Option<String>)| {
            g.painter.stroke_circle(x, y, r, color(c)?, width);
            Ok(())
        });
        methods.add_method_mut("arc", |_, g, (x, y, r, from, to, width, c): (f32, f32, f32, f32, f32, f32, Option<String>)| {
            g.painter.arc(x, y, r, from.to_radians(), to.to_radians(), color(c)?, width);
            Ok(())
        });
        methods.add_method_mut("gradient_arc", |_, g, (x, y, r, from, to, width, start, end): (f32, f32, f32, f32, f32, f32, String, String)| {
            g.painter.gradient_arc(x, y, r, from.to_radians(), to.to_radians(), color(Some(start))?, color(Some(end))?, width);
            Ok(())
        });
        methods.add_method_mut("radial_glow", |_, g, (x, y, radius, steps): (f32, f32, f32, u32)| {
            g.painter.radial_glow(x, y, radius, steps);
            Ok(())
        });
        methods.add_method_mut("line", |_, g, (x1, y1, x2, y2, width, c, round): (f32, f32, f32, f32, f32, Option<String>, Option<bool>)| {
            g.painter.line(x1, y1, x2, y2, color(c)?, width, round.unwrap_or(false));
            Ok(())
        });
        methods.add_method_mut("lines", |_, g, (coords, width, c): (Vec<f32>, f32, Option<String>)| {
            if coords.len() % 4 != 0 {
                return Err(mlua::Error::runtime("lines takes x1, y1, x2, y2 for each segment"));
            }
            let segments: Vec<[f32; 4]> = coords.chunks(4).map(|s| [s[0], s[1], s[2], s[3]]).collect();
            g.painter.lines(&segments, color(c)?, width);
            Ok(())
        });
        methods.add_method_mut("rect", |_, g, (x, y, w, h, c, radius): (f32, f32, f32, f32, Option<String>, Option<f32>)| {
            g.painter.rect(x, y, w, h, radius.unwrap_or(0.0), color(c)?);
            Ok(())
        });
        methods.add_method_mut("polygon", |_, g, (coords, c): (Vec<f32>, Option<String>)| {
            if coords.len() < 6 || coords.len() % 2 != 0 {
                return Err(mlua::Error::runtime("polygon needs at least three x, y pairs"));
            }
            let points: Vec<(f32, f32)> = coords.chunks(2).map(|p| (p[0], p[1])).collect();
            g.painter.polygon(&points, color(c)?);
            Ok(())
        });
        // A whole text stock has is shown in the language in use (as `tr`),
        // in capitals when the skin wrote it in capitals.
        methods.add_method_mut("text", |_, g, (text, x, y, s): (String, f32, f32, Option<Table>)| {
            Ok(g.painter.text(&shown(&text), x, y, &style(s)?))
        });
        methods.add_method_mut("measure", |_, g, (text, s): (String, Option<Table>)| {
            Ok(g.painter.measure(&shown(&text), &style(s)?))
        });
        // Whether the font has the text's every character, as a unit
        // font's symbols, not in a stand-in.
        methods.add_method_mut("has_glyphs", |_, g, (text, s): (String, Option<Table>)| {
            Ok(g.painter.has_glyphs(&text, &style(s)?))
        });
        // Whether the image was drawn: one that cannot be loaded (a unit
        // image another unit lacks) leaves its place empty.
        methods.add_method_mut("image", |_, g, (file, x, y, w, h, alpha): (String, f32, f32, f32, f32, Option<f32>)| {
            let file = image_file(g.dir, &file).map_err(mlua::Error::runtime)?;
            Ok(g.painter.image(&file, x, y, w, h, alpha.unwrap_or(1.0)).is_ok())
        });
        // A frame of a sprite sheet; returns the sheet's frame count, or
        // nothing when the image cannot be loaded (a unit image off the unit).
        methods.add_method_mut("sprite", |_, g, (file, frame, columns, w, h, x, y): (String, u32, u32, f32, f32, f32, f32)| {
            let file = image_file(g.dir, &file).map_err(mlua::Error::runtime)?;
            Ok(g.painter.sprite(&file, frame, columns, w, h, x, y).ok())
        });
        // Another frame in `seconds`, the soonest asked for in a frame.
        methods.add_method_mut("wake", |_, g, seconds: f64| {
            g.wake.set(Some(g.wake.get().map_or(seconds, |w| w.min(seconds))));
            Ok(())
        });
        methods.add_method_mut("save", |_, g, ()| { g.painter.save(); Ok(()) });
        methods.add_method_mut("restore", |_, g, ()| { g.painter.restore(); Ok(()) });
        methods.add_method_mut("translate", |_, g, (x, y): (f32, f32)| { g.painter.translate(x, y); Ok(()) });
        methods.add_method_mut("rotate", |_, g, degrees: f32| { g.painter.rotate(degrees.to_radians()); Ok(()) });
        methods.add_method_mut("scale", |_, g, (x, y): (f32, Option<f32>)| { g.painter.scale(x, y.unwrap_or(x)); Ok(()) });
        methods.add_method_mut("alpha", |_, g, a: f32| { g.painter.alpha(a); Ok(()) });
        methods.add_method_mut("clear", |_, g, c: Option<String>| { g.painter.clear(color(c)?); Ok(()) });
        methods.add_method_mut("layer", |lua, g, (name, key, draw, w, h): (String, LuaValue, Function, Option<f32>, Option<f32>)| {
            let key = match key {
                LuaValue::String(s) => s.to_str()?.to_string(),
                LuaValue::Integer(n) => n.to_string(),
                LuaValue::Number(n) => n.to_string(),
                LuaValue::Boolean(b) => b.to_string(),
                _ => return Err(mlua::Error::runtime("layer keys are strings, numbers or booleans")),
            };
            let (dir, wake) = (g.dir, g.wake);
            let size = match (w, h) {
                (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some((w, h)),
                (None, None) => None,
                _ => return Err(mlua::Error::runtime("a layer's size is a positive width and height")),
            };
            g.painter.layer(&name, &key, size, &mut |painter| {
                lua.scope(|scope| draw.call::<()>(scope.create_userdata(Gfx { painter, dir, wake })?)).map_err(|e| e.to_string())
            }).map_err(mlua::Error::runtime)
        });
        methods.add_method_mut("paint_layer", |_, g, (name, c, clip): (String, Option<String>, Option<Vec<f32>>)| {
            let points: Option<Vec<(f32, f32)>> = clip.map(|c| c.chunks(2).filter(|p| p.len() == 2).map(|p| (p[0], p[1])).collect());
            if points.as_ref().is_some_and(|p| p.len() < 3) {
                return Err(mlua::Error::runtime("paint_layer's clip needs at least three x, y pairs"));
            }
            g.painter.paint_layer(&name, color(c)?, points.as_deref()).map_err(mlua::Error::runtime)
        });
        methods.add_method_mut("warp_layer", |_, g, (name, c): (String, Vec<f32>)| {
            let [x0, y0, x1, y1, x2, y2, x3, y3] = c[..] else {
                return Err(mlua::Error::runtime("warp_layer's corners are four x, y pairs"));
            };
            g.painter.warp_layer(&name, &[(x0, y0), (x1, y1), (x2, y2), (x3, y3)]).map_err(mlua::Error::runtime)
        });
    }
}

/// Everything a skin's `draw` reads, as the `ui` table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ui {
    /// Seconds since the UI started.
    pub now: f64,
    /// "home" or "menu".
    pub screen: &'static str,
    /// The face in the skin's design units and in pixels.
    pub design: f32,
    pub face_px: f32,
    pub thermostat: ThermostatView,
    pub menu: Option<crate::menu::View>,
    /// The alert shown in place of everything else, when one is up.
    pub alert: Option<String>,
    /// Farsight shows, or gives way to the home screen, under the screen:
    /// an alert, notice or dialog over it.
    pub far: bool,
    /// A notice with CONTINUE, when one is up.
    pub notice: Option<&'static str>,
    /// An error page, and whether its TECH INFO is highlighted.
    /// The error page, and its choice highlighted once the wheel turns.
    pub error: Option<(ErrorPage, Option<usize>)>,
    /// The fan timer, while it runs.
    pub fan: Option<FanView>,
    /// The fan runs, for its timer or its schedule.
    pub fan_on: bool,
    /// The fan schedule as the top menu's Fan words it, while it is on.
    pub fan_schedule: Option<String>,
    pub dialog: Option<Dialog>,
    /// The system test's page, while it is open.
    pub test: Option<crate::systemtest::View>,
    /// The Schedule screen, while it is open.
    pub schedule: Option<ScheduleView>,
    pub history: Option<HistoryView>,
    pub keyboard: Option<KeyboardView>,
    pub postal: Option<PostalView>,
    pub wifi: Option<WifiView>,
    pub clock: Clock,
    /// When things last changed, in `now`'s seconds, for animations.
    pub changed_screen: f64,
    pub changed_target: f64,
    /// When the wheel last moved the target.
    pub turned_target: f64,
    pub changed_menu: f64,
    /// When Farsight's near view starts coming in, while someone is near.
    pub changed_near: Option<f64>,
    /// When Farsight started giving way to the home screen, while it does:
    /// its clocks or weather fading out, its temperatures turning into the
    /// home screen's.
    pub changed_leave: Option<f64>,
}

/// The fan timer while it runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FanView {
    /// "29 minutes left", "3 days left"; with a fan of more than one
    /// speed, "Running for the next 29 minutes at low speed".
    pub left_text: String,
    /// The share of its run left, 1 to 0.
    pub left: f64,
    /// "Until 03:30", or "Until 03:07 at Friday" a day or more away; "On
    /// low until 03:30" with a fan of more than one speed.
    pub until: String,
    /// A fan of more than one speed: the page says it in a sentence.
    pub speed: bool,
}

/// Stock's error page: a code in a badge, a title, the text, a link and
/// the choices under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ErrorPage {
    pub code: &'static str,
    pub title: &'static str,
    pub text: &'static str,
    pub link: &'static str,
    pub choices: &'static [&'static str],
}

/// The Schedule screen: minutes count from Monday 00:00, temperatures
/// are in the shown unit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScheduleView {
    /// The cursor's minute; before 0 it is on a home dial.
    pub cursor: i32,
    /// The setpoint under the cursor, by its minute.
    pub held: Option<u32>,
    /// Where the timeline sits under the cursor, in minutes, as the wheel
    /// flung it.
    pub scroll: f64,
    /// Setpoints: minute, temperature (a range's low) and a range's high.
    pub points: Vec<(u32, f32, Option<f32>)>,
    /// The temperature at each midnight's triangle, Monday's first and
    /// the week's end last; none where stock leaves it out.
    pub starts: Vec<Option<f32>>,
    /// The setpoint being made or changed, and which of its "time" or
    /// "temp" (or a range's "high") the wheel sets.
    pub pending: Option<(u32, f32, Option<f32>)>,
    pub editing: Option<&'static str>,
    /// A menu over the timeline: its choices and the highlighted one.
    pub menu: Option<(Vec<&'static str>, usize)>,
    /// The current minute.
    pub now: u32,
}

/// The History screen: its days oldest first, the slot the wheel is on
/// (0 and days + 1 are the live thermostat), and a day's detail page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HistoryView {
    pub at: usize,
    /// Where the cards are, in slots, as the wheel flung them.
    pub pos: f64,
    pub days: Vec<HistoryDay>,
    /// Heat, cool or both shown: -1 neither, 0 heat, 1 cool, 2 both.
    pub usage: i8,
    pub detail: Option<HistoryDetail>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HistoryDay {
    pub title: String,
    pub leaf: bool,
    pub missing: bool,
    pub heat: u32,
    pub cool: u32,
    pub heat_text: String,
    pub cool_text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HistoryDetail {
    pub scroll: u8,
    pub heat_text: String,
    pub cool_text: String,
    /// Runs: start and length in seconds of the day, heat or cool.
    pub cycles: Vec<(u32, u32, bool)>,
    /// The reason's symbol (stock's energy_winner), when one is shown.
    pub reason: Option<i8>,
    pub message: String,
}

/// A Wi-Fi page: connecting (its phase text, STOP CONNECTING highlighted
/// or not), online, failed (its text, help link and hint), the hidden
/// network's security question, or disconnecting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WifiView {
    pub page: &'static str,
    pub name: String,
    pub text: String,
    pub help: String,
    pub hint: String,
    pub stop: bool,
    pub choices: Vec<&'static str>,
    pub at: usize,
}

/// The keyboard: its keys in ring order (characters, or shift, space,
/// delete, page, done, cancel), the one selected, the page, the text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyboardView {
    pub keys: Vec<String>,
    pub at: usize,
    pub page: usize,
    pub text: String,
    pub placeholder: String,
    pub page_label: &'static str,
}

/// Location's postal-code boxes: each one's character (`-` while not
/// set), the box being set (past the last, DONE), and a gap's place.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PostalView {
    pub prompt: String,
    pub chars: Vec<String>,
    pub set: Vec<bool>,
    pub at: usize,
    pub gap: Option<usize>,
}

/// The local time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clock {
    /// 0 is Sunday.
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub twelve_hour: bool,
    /// While someone is near Farsight: its digital clock's and its analog
    /// clock's date lines, "Saturday, October 17" and "SAT 17" in English.
    pub weekday_date: Option<String>,
    pub weekday_day: Option<String>,
}

/// The thermostat in the unit it is shown in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThermostatView {
    pub target: f32,
    pub current: f32,
    pub min: f32,
    pub max: f32,
    pub unit: &'static str,
    pub mode: String,
    pub hvac: &'static str,
    /// The stage it runs, 1 to 3, and whether Cool to Dry runs it.
    pub stage: u8,
    pub drying: bool,
    /// Emergency heat is the mode; auxiliary heat runs.
    pub emergency: bool,
    pub aux: bool,
    /// Alternate heat running: its stage, 0 when it is not.
    pub alt: u8,
    /// Airwave cooling with the fan.
    pub airwave: bool,
    /// Early-On running: "heat" or "cool".
    pub preconditioning: Option<&'static str>,
    /// The room near a safety temperature nothing else acts on.
    pub safety: bool,
    /// HvacBatteryDrained: heating with power stealing off has run the
    /// battery down.
    pub system_issue: bool,
    /// The target at its mode's far end on a system that also does the
    /// other: "cool" in heat at or under 10 °C (51 °F), "heat" in cool at
    /// or over 31 °C (89 °F). A press then opens the menus on Mode.
    pub press_to: Option<&'static str>,
    /// The leaf's thresholds in the shown unit: a heat target at or under
    /// the first, or a cool target at or over the second, earns it; and
    /// whether it is earned now (off and Eco always are).
    pub leaf: (f32, f32),
    pub leaf_on: bool,
    /// How far the ring has turned towards its next step, in the shown
    /// unit: stock's dial follows every detent, the target every step.
    pub nudge: f32,
    /// Sunblock is correcting (`sunlight_correction_active`).
    pub sunblock: bool,
    /// The outside temperature in the shown unit, and stock's picture for
    /// the weather, when nestlocal gave them.
    pub outdoor: Option<f32>,
    pub weather_image: Option<&'static str>,
    /// Wi-Fi is connected.
    pub online: bool,
    /// While someone is near: the three hours Farsight's weather forecasts,
    /// their temperature as shown and their time.
    pub forecast: Vec<(f32, String)>,
    /// Minutes until the room reaches the target, while one is known.
    pub time_to_target: Option<u32>,
    /// The heat-cool range's ends.
    pub low: f32,
    pub high: f32,
    /// Relative humidity in percent, when known.
    pub humidity: Option<f32>,
    /// What the wiring can run.
    pub can_heat: bool,
    pub can_cool: bool,
    /// In heat-cool, the end of the range picked on the home screen
    /// ("low" or "high"), and whether it is being changed.
    pub range: Option<&'static str>,
    pub range_editing: bool,
    /// Eco is on: `target` is the Eco temperature.
    pub eco: bool,
    /// Seconds until a held-back system starts.
    pub starts_in: Option<u64>,
    /// Seconds a held-on system runs on before it stops.
    pub delayed_for: Option<u64>,
    /// The lockout `starts_in` or `delayed_for` counts: "compressor" or
    /// "cycle".
    pub lockout: Option<&'static str>,
    /// What Farsight shows: "target_temp", "current_temp", "analog_clock",
    /// "digital_clock" or "weather".
    pub farsight: &'static str,
}

/// A question with its answers, over everything but an alert.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dialog {
    pub text: String,
    pub options: &'static [&'static str],
    pub selected: usize,
    /// A symbol over the text (stock's Wi-Fi alerts).
    pub icon: Option<&'static str>,
    /// A heading over the text, and a link under it (stock's HVAC
    /// alerts).
    pub title: Option<&'static str>,
    pub link: Option<&'static str>,
    /// The answer ticked as the one in force (stock's flag 8).
    pub checked: Option<usize>,
    /// Lines a page taller than the face is turned down.
    pub scrolled: usize,
}

impl Ui {
    /// Whether two views differ in nothing but the time.
    fn same_but_now(&self, o: &Ui) -> bool {
        self.screen == o.screen && self.design == o.design && self.face_px == o.face_px
            && self.thermostat == o.thermostat && self.menu == o.menu && self.clock == o.clock
            && self.alert == o.alert && self.far == o.far && self.fan == o.fan && self.fan_schedule == o.fan_schedule && self.dialog == o.dialog && self.test == o.test
            && self.changed_screen == o.changed_screen && self.changed_target == o.changed_target
            && self.turned_target == o.turned_target && self.changed_menu == o.changed_menu && self.changed_near == o.changed_near && self.changed_leave == o.changed_leave
            && self.schedule == o.schedule && self.history == o.history && self.keyboard == o.keyboard && self.postal == o.postal && self.wifi == o.wifi
    }

    fn to_lua(&self, lua: &Lua) -> mlua::Result<Table> {
        let ui = lua.create_table()?;
        ui.set("now", self.now)?;
        ui.set("screen", self.screen)?;
        ui.set("alert", self.alert.as_deref())?;
        ui.set("far", self.far)?;
        ui.set("notice", self.notice)?;
        if let Some((e, tech)) = &self.error {
            let error = lua.create_table()?;
            error.set("code", e.code)?;
            error.set("title", e.title)?;
            error.set("text", e.text)?;
            error.set("link", e.link)?;
            error.set("choices", e.choices.to_vec())?;
            error.set("selected", tech.map(|c| c + 1))?;
            ui.set("error", error)?;
        }
        if let Some(d) = &self.dialog {
            let dialog = lua.create_table()?;
            dialog.set("text", d.text.as_str())?;
            dialog.set("icon", d.icon)?;
            dialog.set("title", d.title)?;
            dialog.set("link", d.link)?;
            dialog.set("options", d.options.to_vec())?;
            dialog.set("selected", d.selected + 1)?;
            dialog.set("checked", d.checked.map(|c| c + 1))?;
            dialog.set("scrolled", d.scrolled)?;
            ui.set("dialog", dialog)?;
        }
        if let Some(v) = &self.test {
            let test = lua.create_table()?;
            test.set("heading", v.heading.clone())?;
            test.set("text", v.text.as_str())?;
            test.set("link", v.link.as_deref())?;
            test.set("options", v.options.clone())?;
            test.set("ticked", v.ticked.clone())?;
            test.set("selected", v.selected + 1)?;
            ui.set("test", test)?;
        }
        ui.set("fan_on", self.fan_on)?;
        ui.set("fan_schedule", self.fan_schedule.as_deref())?;
        if let Some(f) = &self.fan {
            let fan = lua.create_table()?;
            fan.set("left_text", f.left_text.as_str())?;
            fan.set("left", f.left)?;
            fan.set("ends", f.until.as_str())?;
            fan.set("speed", f.speed)?;
            ui.set("fan", fan)?;
        }
        let face = lua.create_table()?;
        face.set("size", self.design)?;
        face.set("px", self.face_px)?;
        face.set("scale", self.face_px / self.design)?;
        ui.set("face", face)?;
        let t = lua.create_table()?;
        t.set("target", self.thermostat.target)?;
        t.set("current", self.thermostat.current)?;
        t.set("min", self.thermostat.min)?;
        t.set("max", self.thermostat.max)?;
        t.set("unit", self.thermostat.unit)?;
        t.set("mode", self.thermostat.mode.as_str())?;
        t.set("hvac", self.thermostat.hvac)?;
        t.set("stage", self.thermostat.stage)?;
        t.set("drying", self.thermostat.drying)?;
        t.set("emergency", self.thermostat.emergency)?;
        t.set("aux", self.thermostat.aux)?;
        t.set("alt", self.thermostat.alt)?;
        t.set("airwave", self.thermostat.airwave)?;
        t.set("preconditioning", self.thermostat.preconditioning)?;
        t.set("safety", self.thermostat.safety)?;
        t.set("system_issue", self.thermostat.system_issue)?;
        t.set("press_to", self.thermostat.press_to)?;
        t.set("leaf_heat", self.thermostat.leaf.0)?;
        t.set("leaf_cool", self.thermostat.leaf.1)?;
        t.set("leaf_on", self.thermostat.leaf_on)?;
        t.set("nudge", self.thermostat.nudge)?;
        t.set("sunblock", self.thermostat.sunblock)?;
        t.set("outdoor", self.thermostat.outdoor)?;
        t.set("time_to_target", self.thermostat.time_to_target)?;
        t.set("weather_image", self.thermostat.weather_image)?;
        t.set("online", self.thermostat.online)?;
        let forecast = lua.create_table()?;
        for (i, (temp, time)) in self.thermostat.forecast.iter().enumerate() {
            let hour = lua.create_table()?;
            hour.set("temp", *temp)?;
            hour.set("time", time.as_str())?;
            forecast.set(i + 1, hour)?;
        }
        t.set("forecast", forecast)?;
        t.set("low", self.thermostat.low)?;
        t.set("high", self.thermostat.high)?;
        t.set("humidity", self.thermostat.humidity)?;
        t.set("can_heat", self.thermostat.can_heat)?;
        t.set("can_cool", self.thermostat.can_cool)?;
        t.set("range", self.thermostat.range)?;
        t.set("range_editing", self.thermostat.range_editing)?;
        t.set("eco", self.thermostat.eco)?;
        t.set("starts_in", self.thermostat.starts_in)?;
        t.set("delayed_for", self.thermostat.delayed_for)?;
        t.set("lockout", self.thermostat.lockout)?;
        t.set("farsight", self.thermostat.farsight)?;
        ui.set("thermostat", t)?;
        if let Some(view) = &self.menu {
            let m = lua.create_table()?;
            m.set("title", view.title.as_str())?;
            m.set("path", view.path.clone())?;
            m.set("depth", view.depth)?;
            m.set("selected", view.selected + 1)?;
            m.set("editing", view.editing)?;
            m.set("turned", view.turned)?;
            if let Some(scroll) = view.scroll {
                m.set("scroll", scroll)?;
            }
            m.set("travel", view.travel)?;
            m.set("page", view.page)?;
            m.set("pending", view.pending.map(|p| p + 1))?;
            m.set("icon", view.icon.as_deref())?;
            m.set("style", view.style.as_deref())?;
            m.set("prompt", view.prompt.as_deref())?;
            m.set("detail", view.detail.as_deref())?;
            m.set("intro", view.intro)?;
            m.set("list_at", view.list_at)?;
            m.set("image", view.image.as_deref())?;
            m.set("image_w", view.image_size.map(|s| s.0))?;
            m.set("image_h", view.image_size.map(|s| s.1))?;
            if let Some(f) = &view.form {
                let form = lua.create_table()?;
                form.set("prompt", f.prompt.as_deref())?;
                form.set("icon", f.icon.as_deref())?;
                let fields = lua.create_table()?;
                for (i, (label, v, unit)) in f.fields.iter().enumerate() {
                    let e = lua.create_table()?;
                    e.set("label", label.as_str())?;
                    e.set("value", *v)?;
                    e.set("unit", unit.as_deref())?;
                    e.set("text", f.texts.get(i).and_then(|t| t.as_deref()))?;
                    fields.set(i + 1, e)?;
                }
                form.set("fields", fields)?;
                form.set("style", f.style.as_deref())?;
                form.set("buttons", lua.create_sequence_from(f.buttons.iter().copied())?)?;
                form.set("at", f.at + 1)?;
                form.set("changing", f.changing)?;
                m.set("form", form)?;
            }
            if let Some(page) = &view.info {
                let info = lua.create_table()?;
                info.set("text", page.text.as_str())?;
                info.set("link", page.link.as_deref())?;
                info.set("button", page.button.to_uppercase())?;
                info.set("icon", page.icon.as_deref())?;
                info.set("scrolled", page.scrolled)?;
                info.set("style", page.style.as_deref())?;
                info.set("wait", page.wait)?;
                info.set("image", page.image.as_deref())?;
                info.set("busy", page.busy)?;
                m.set("info", info)?;
                m.set("info_top", view.depth == 0)?;
            }
            let rows = lua.create_table()?;
            for (i, row) in view.rows.iter().enumerate() {
                let r = lua.create_table()?;
                r.set("label", row.label.as_str())?;
                r.set("icon", row.icon.as_deref())?;
                r.set("style", row.style.as_deref())?;
                r.set("ticked", row.ticked)?;
                if !row.summary.is_empty() {
                    let sum = lua.create_table()?;
                    for (i, (short, v)) in row.summary.iter().enumerate() {
                        let e = lua.create_table()?;
                        e.set("short", short.as_str())?;
                        e.set("value", *v)?;
                        sum.set(i + 1, e)?;
                    }
                    r.set("summary", sum)?;
                }
                if !row.detail.is_empty() {
                    let detail = lua.create_table()?;
                    for (i, (label, value)) in row.detail.iter().enumerate() {
                        let e = lua.create_table()?;
                        e.set("label", label.as_str())?;
                        e.set("value", value.as_str())?;
                        detail.set(i + 1, e)?;
                    }
                    r.set("detail", detail)?;
                }
                r.set("prompt", row.prompt.as_deref())?;
                r.set("fraction", row.fraction)?;
                r.set("kind", row.kind)?;
                r.set("tile", row.tile.as_deref())?;
                r.set("value", row.value.as_deref())?;
                r.set("editing", row.editing)?;
                if !row.options.is_empty() {
                    let options = lua.create_table()?;
                    for (j, o) in row.options.iter().enumerate() {
                        let entry = lua.create_table()?;
                        entry.set("value", o.value.as_str())?;
                        entry.set("label", o.label.as_str())?;
                        options.set(j + 1, entry)?;
                    }
                    r.set("options", options)?;
                    r.set("saved", row.saved.map(|s| s + 1))?;
                }
                rows.set(i + 1, r)?;
            }
            m.set("items", rows)?;
            ui.set("menu", m)?;
        }
        if let Some(sv) = &self.schedule {
            let sch = lua.create_table()?;
            sch.set("cursor", sv.cursor)?;
            sch.set("held", sv.held)?;
            sch.set("scroll", sv.scroll)?;
            let points = lua.create_table()?;
            for (i, (minute, temp, high)) in sv.points.iter().enumerate() {
                let p = lua.create_table()?;
                p.set("minute", *minute)?;
                p.set("temp", *temp)?;
                p.set("high", *high)?;
                points.set(i + 1, p)?;
            }
            sch.set("points", points)?;
            let starts = lua.create_table()?;
            for (i, t) in sv.starts.iter().enumerate() {
                starts.set(i + 1, *t)?;
            }
            sch.set("starts", starts)?;
            if let Some((minute, temp, high)) = sv.pending {
                let p = lua.create_table()?;
                p.set("minute", minute)?;
                p.set("temp", temp)?;
                p.set("high", high)?;
                sch.set("pending", p)?;
            }
            sch.set("editing", sv.editing)?;
            if let Some((choices, at)) = &sv.menu {
                sch.set("choices", choices.clone())?;
                sch.set("choice", *at + 1)?;
            }
            sch.set("now", sv.now)?;
            ui.set("schedule", sch)?;
        }
        if let Some(hv) = &self.history {
            let h = lua.create_table()?;
            h.set("at", hv.at)?;
            h.set("pos", hv.pos)?;
            h.set("usage", hv.usage)?;
            let days = lua.create_table()?;
            for (i, d) in hv.days.iter().enumerate() {
                let t = lua.create_table()?;
                t.set("title", d.title.as_str())?;
                t.set("leaf", d.leaf)?;
                t.set("missing", d.missing)?;
                t.set("heat", d.heat)?;
                t.set("cool", d.cool)?;
                t.set("heat_text", d.heat_text.as_str())?;
                t.set("cool_text", d.cool_text.as_str())?;
                days.set(i + 1, t)?;
            }
            h.set("days", days)?;
            if let Some(dv) = &hv.detail {
                let t = lua.create_table()?;
                t.set("scroll", dv.scroll)?;
                t.set("heat_text", dv.heat_text.as_str())?;
                t.set("cool_text", dv.cool_text.as_str())?;
                let cycles = lua.create_table()?;
                for (i, (start, length, heat)) in dv.cycles.iter().enumerate() {
                    let c = lua.create_table()?;
                    c.set("start", *start)?;
                    c.set("length", *length)?;
                    c.set("heat", *heat)?;
                    cycles.set(i + 1, c)?;
                }
                t.set("cycles", cycles)?;
                t.set("reason", dv.reason)?;
                t.set("message", dv.message.as_str())?;
                h.set("detail", t)?;
            }
            ui.set("history", h)?;
        }
        if let Some(w) = &self.wifi {
            let t = lua.create_table()?;
            t.set("page", w.page)?;
            t.set("name", w.name.as_str())?;
            t.set("text", w.text.as_str())?;
            t.set("help", w.help.as_str())?;
            t.set("hint", w.hint.as_str())?;
            t.set("stop", w.stop)?;
            t.set("choices", w.choices.clone())?;
            t.set("at", w.at + 1)?;
            ui.set("wifi", t)?;
        }
        if let Some(kv) = &self.keyboard {
            let k = lua.create_table()?;
            k.set("keys", kv.keys.clone())?;
            k.set("at", kv.at + 1)?;
            k.set("page", kv.page)?;
            k.set("text", kv.text.as_str())?;
            k.set("placeholder", kv.placeholder.as_str())?;
            k.set("page_label", kv.page_label)?;
            ui.set("keyboard", k)?;
        }
        if let Some(pv) = &self.postal {
            let p = lua.create_table()?;
            p.set("prompt", pv.prompt.as_str())?;
            p.set("chars", pv.chars.clone())?;
            p.set("set", pv.set.clone())?;
            p.set("at", pv.at + 1)?;
            p.set("gap", pv.gap)?;
            ui.set("postal", p)?;
        }
        let clock = lua.create_table()?;
        clock.set("weekday", self.clock.weekday)?;
        clock.set("hour", self.clock.hour)?;
        clock.set("minute", self.clock.minute)?;
        clock.set("second", self.clock.second)?;
        clock.set("twelve_hour", self.clock.twelve_hour)?;
        clock.set("weekday_date", self.clock.weekday_date.as_deref())?;
        clock.set("weekday_day", self.clock.weekday_day.as_deref())?;
        ui.set("clock", clock)?;
        let changed = lua.create_table()?;
        changed.set("screen", self.changed_screen)?;
        changed.set("target", self.changed_target)?;
        changed.set("turned", self.turned_target)?;
        changed.set("menu", self.changed_menu)?;
        changed.set("near", self.changed_near)?;
        changed.set("leave", self.changed_leave)?;
        ui.set("changed", changed)?;
        Ok(ui)
    }
}

/// The skins in folder `dir` that load, as their folders' and their own
/// names, by folder.
pub fn catalog(dir: &Path) -> Vec<(String, String)> {
    let mut skins: Vec<(String, String)> = std::fs::read_dir(dir).into_iter().flatten().flatten()
        .filter(|e| e.path().join("skin.lua").is_file())
        .filter_map(|e| {
            let folder = e.file_name().to_string_lossy().into_owned();
            match Skin::load(&e.path()) {
                Ok(skin) => {
                    debug!("skins: {folder} ({})", skin.name);
                    Some((folder, skin.name))
                }
                Err(err) => {
                    warn!("skins: {folder} left out: {err}");
                    None
                }
            }
        })
        .collect();
    skins.sort();
    skins
}

/// Reads a menus file: the tree and each setting's default. A choice whose
/// `options` is `"skins"` offers `skins`, its default first.
pub fn load_menus(path: &Path, skins: &[(String, String)]) -> Result<(Tree, Vec<(String, Value)>), String> {
    load_menus_with(path, skins, &drop_in())
}

/// As load_menus, with plugins dropped into `drop_in` as well as those
/// beside the menus file.
pub fn load_menus_with(path: &Path, skins: &[(String, String)], drop_in: &Path) -> Result<(Tree, Vec<(String, Value)>), String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let (lua, deadline) = sandbox(dir).map_err(|e| e.to_string())?;
    let source = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root: Table = limited(&deadline, || lua.load(source).set_name("@menus.lua").eval()).map_err(|e| e.to_string())?;
    let mut defaults = Vec::new();
    let title = root.get::<Option<String>>("title").map_err(|e| e.to_string())?.unwrap_or_else(|| "Settings".into());
    let back = root.get::<Option<String>>("back").map_err(|e| e.to_string())?.unwrap_or_else(|| "Back".into());
    let mut items = items_of(&root, &title, skins, &mut defaults)?;
    let dirs = vec![dir.join(PLUGINS), drop_in.to_path_buf()];
    PLUGIN_DIRS.with(|held| held.borrow_mut().clone_from(&dirs));
    let values = plugins(&lua, &deadline, &dirs, &mut items, skins, &mut defaults)?;
    debug!("menus: {}: {} entries at the top, {} defaults, {} plugin values", crate::log::path(path), items.len(), defaults.len(), values.len());
    Ok((Tree { title, back, items, values }, defaults))
}

/// Where plugins sit, beside the menus file.
pub const PLUGINS: &str = "plugins";
/// Where the system drops plugins of its own, one a feature it installs
/// (unfr0zen's Matter, Zigbee, Doom pages), apart from the client's files.
pub const DROP_IN: &str = "/etc/unfr0zen/plugins";
static DROP_IN_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
thread_local! {
    /// The folders plugins were last read from, for `plugin:` pictures:
    /// menus are read and skins drawn on the one thread.
    static PLUGIN_DIRS: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Reads dropped-in plugins from `dir` rather than DROP_IN.
pub fn set_drop_in(dir: PathBuf) {
    let _ = DROP_IN_DIR.set(dir);
}

/// The folder plugins are dropped into.
pub fn drop_in() -> PathBuf {
    DROP_IN_DIR.get().cloned().unwrap_or_else(|| PathBuf::from(DROP_IN))
}

/// Applies the plugins in `dirs` (each a `.lua` file) to the menus, all in
/// name order, one in a later folder in place of one of the same name in
/// an earlier. Each plugin's entries follow the previous plugin's in the
/// menu they go to. A plugin returns a table of:
///   `hide`: labels of entries to leave out of the menu it adds to;
///   `menus`: entries, as in the menus file, added after that menu's first
///            entry, in order (`under` names the menu: Settings unless set);
///   `values`: settings read from shell commands while the menus are open,
///             `{ ["key"] = "command" }`, for `{key}` in texts and `when`.
/// Returns the values' keys and commands.
fn plugins(lua: &Lua, deadline: &Deadline, dirs: &[PathBuf], items: &mut [Item], skins: &[(String, String)],
           defaults: &mut Vec<(String, Value)>) -> Result<Vec<(String, String)>, String> {
    let mut files = std::collections::BTreeMap::new();
    for dir in dirs {
        for path in std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()) {
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
            if path.extension().is_some_and(|e| e == "lua") && !name.starts_with('.') {
                files.insert(name, path);
            }
        }
    }
    let mut values = Vec::new();
    let mut added = std::collections::HashMap::new();
    for (name, file) in files {
        // A plugin that will not load is left out, the menus kept.
        match plugin(lua, deadline, &file, &name, items, skins, defaults, &mut values, &mut added) {
            Ok(()) => debug!("menus: plugin {name} applied"),
            Err(e) => warn!("menus: plugin {name} left out: {e}"),
        }
    }
    Ok(values)
}

/// Whether two labels are the same, case and line breaks aside.
fn same(a: &str, b: &str) -> bool {
    a.replace('\n', " ").eq_ignore_ascii_case(&b.replace('\n', " "))
}

#[allow(clippy::too_many_arguments)]
fn plugin(lua: &Lua, deadline: &Deadline, file: &Path, name: &str, items: &mut [Item], skins: &[(String, String)],
          defaults: &mut Vec<(String, Value)>, values: &mut Vec<(String, String)>,
          added: &mut std::collections::HashMap<String, usize>) -> Result<(), String> {
    {
        let here = format!("plugin {name}");
        let source = std::fs::read_to_string(file).map_err(|e| format!("{here}: {e}"))?;
        let root: Table = limited(deadline, || lua.load(source).set_name(format!("@{name}")).eval()).map_err(|e| format!("{here}: {e}"))?;
        let under = root.get::<Option<String>>("under").map_err(|e| format!("{here}: under: {e}"))?.unwrap_or_else(|| "Settings".into());
        let Some(Item { kind: Kind::Menu(menu), .. }) = items.iter_mut().find(|i| same(&i.label, &under)) else {
            return Err(format!("{here}: no menu {under:?} to add to"));
        };
        if let Some(hide) = root.get::<Option<Table>>("hide").map_err(|e| format!("{here}: hide: {e}"))? {
            for label in hide.sequence_values::<String>() {
                let label = label.map_err(|e| format!("{here}: hide: {e}"))?;
                menu.retain(|i| !same(&i.label, &label));
            }
        }
        if root.get::<Option<Table>>("menus").map_err(|e| format!("{here}: menus: {e}"))?.is_some() {
            let list = lua.create_table().map_err(|e| e.to_string())?;
            list.set("items", root.get::<Table>("menus").map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let entries = items_of(&list, &here, skins, defaults)?;
            // After the menu's first entry and what earlier plugins added.
            let before = added.entry(under.to_lowercase()).or_insert(0);
            let at = (1 + *before).min(menu.len());
            *before += entries.len();
            menu.splice(at..at, entries);
        }
        if let Some(table) = root.get::<Option<Table>>("values").map_err(|e| format!("{here}: values: {e}"))? {
            for pair in table.pairs::<String, String>() {
                values.push(pair.map_err(|e| format!("{here}: values: {e}"))?);
            }
        }
    }
    Ok(())
}

fn items_of(table: &Table, path: &str, skins: &[(String, String)], defaults: &mut Vec<(String, Value)>) -> Result<Vec<Item>, String> {
    let list: Table = table.get("items").map_err(|e| format!("{path}: items: {e}"))?;
    let mut out = Vec::new();
    for entry in list.sequence_values::<Table>() {
        let t = entry.map_err(|e| format!("{path}: {e}"))?;
        let get = |field: &str| t.get::<LuaValue>(field).map_err(|e| format!("{path}: {field}: {e}"));
        // back = true, or a number of levels to go back up.
        let up = match t.get::<LuaValue>("back").map_err(|e| format!("{path}: back: {e}"))? {
            LuaValue::Boolean(true) => Some(1),
            LuaValue::Integer(n) if n >= 1 => Some(n as usize),
            LuaValue::Nil | LuaValue::Boolean(false) => None,
            other => return Err(format!("{path}: back: true or a number of levels, not {other:?}")),
        };
        let is_back = up.is_some()
            || t.get::<Option<bool>>("done").map_err(|e| format!("{path}: done: {e}"))? == Some(true);
        let heading: Option<String> = t.get("heading").map_err(|e| format!("{path}: heading: {e}"))?;
        let label: String = if let Some(h) = &heading {
            h.clone()
        } else if is_back {
            t.get::<Option<String>>("label").map_err(|e| format!("{path}: label: {e}"))?.unwrap_or_default()
        } else {
            t.get("label").map_err(|e| format!("{path}: an item without a label: {e}"))?
        };
        let here = format!("{path} > {label}");
        let icon: Option<String> = t.get("icon").map_err(|e| format!("{here}: icon: {e}"))?;
        let style: Option<String> = t.get("style").map_err(|e| format!("{here}: style: {e}"))?;
        let prompt: Option<String> = t.get("prompt").map_err(|e| format!("{here}: prompt: {e}"))?;
        let text = |field: &str| -> Result<Option<String>, String> {
            t.get::<Option<String>>(field).map_err(|e| format!("{here}: {field}: {e}"))
        };
        let number = |field: &str| -> Result<f64, String> {
            t.get::<f64>(field).map_err(|e| format!("{here}: {field}: {e}"))
        };
        let kind = if heading.is_some() {
            Kind::Heading
        } else if let Some(form) = t.get::<Option<Table>>("form").map_err(|e| format!("{here}: form: {e}"))? {
            let mut fields = Vec::new();
            for f in form.sequence_values::<Table>() {
                let f = f.map_err(|e| format!("{here}: form: {e}"))?;
                let get_text = |k: &str| f.get::<Option<String>>(k).map_err(|e| format!("{here}: form {k}: {e}"));
                let get_num = |k: &str| f.get::<f64>(k).map_err(|e| format!("{here}: form {k}: {e}"));
                let key = get_text("key")?.ok_or_else(|| format!("{here}: a form field needs a key"))?;
                let off = match get_text("off")?.as_deref() {
                    Some("below") => Some(Off::Below),
                    Some("above") => Some(Off::Above),
                    None => None,
                    Some(o) => return Err(format!("{here}: form off {o:?}: below or above")),
                };
                match f.get::<LuaValue>("default").map_err(|e| format!("{here}: form default: {e}"))? {
                    LuaValue::Number(n) => defaults.push((key.clone(), Value::Number(n))),
                    LuaValue::Integer(n) => defaults.push((key.clone(), Value::Number(n as f64))),
                    LuaValue::String(_) => defaults.push((key.clone(), Value::Text("off".into()))),
                    _ => {}
                }
                let mut options = Vec::new();
                if let Some(list) = f.get::<Option<Table>>("options").map_err(|e| format!("{here}: form options: {e}"))? {
                    for o in list.sequence_values::<Table>() {
                        let o = o.map_err(|e| format!("{here}: form options: {e}"))?;
                        let value: String = o.get(1).map_err(|e| format!("{here}: form option value: {e}"))?;
                        let label: Option<String> = o.get(2).map_err(|e| format!("{here}: form option label: {e}"))?;
                        options.push(Choice { label: label.unwrap_or_else(|| value.clone()), value });
                    }
                    if options.is_empty() {
                        return Err(format!("{here}: form options: none given"));
                    }
                }
                if !options.is_empty() {
                    if let Some(i) = defaults.iter().position(|(k, _)| *k == key) {
                        defaults.remove(i);
                    }
                    let default = get_text("default")?.unwrap_or_else(|| options[0].value.clone());
                    defaults.push((key.clone(), Value::Text(default)));
                    fields.push(Field { label: get_text("label")?.unwrap_or_default(), short: get_text("short")?.unwrap_or_default(), key,
                                        min: 0.0, max: options.len() as f64 - 1.0, step: 1.0, off: None, unit: None, options });
                    continue;
                }
                fields.push(Field { label: get_text("label")?.unwrap_or_default(), short: get_text("short")?.unwrap_or_default(), key,
                                    min: get_num("min")?, max: get_num("max")?, step: get_num("step")?, off, unit: get_text("unit")?,
                                    options: Vec::new() });
            }
            let more_first = t.get::<Option<bool>>("more_first").map_err(|e| format!("{here}: more_first: {e}"))?.unwrap_or(false);
            let enable = text("enable")?;
            if let Some(key) = &enable {
                defaults.push((key.clone(), Value::Text("off".into())));
            }
            Kind::Form { fields, more: text("more")?, link: text("link")?, more_first, enable, action: text("action")? }
        } else if let Some(lines) = t.get::<Option<Table>>("section").map_err(|e| format!("{here}: section: {e}"))? {
            let mut out = Vec::new();
            for line in lines.sequence_values::<Table>() {
                let line = line.map_err(|e| format!("{here}: section: {e}"))?;
                let label: String = line.get(1).map_err(|e| format!("{here}: section label: {e}"))?;
                let value: String = line.get(2).map_err(|e| format!("{here}: section value: {e}"))?;
                out.push((label, value));
            }
            Kind::Section(out)
        } else if let Some(key) = text("set")? {
            let value = text("value")?.ok_or_else(|| format!("{here}: set needs a value"))?;
            Kind::Set { key, value, confirm: text("confirm")? }
        } else if let Some(n) = up {
            if n == 1 { Kind::Back } else { Kind::Up(n) }
        } else if t.get::<Option<bool>>("done").map_err(|e| format!("{here}: done: {e}"))? == Some(true) {
            Kind::Done
        } else if !get("items")?.is_nil() {
            Kind::Menu(items_of(&t, &here, skins, defaults)?)
        } else if let Some(key) = text("choice")? {
            let mut choices = Vec::new();
            match get("options")? {
                LuaValue::String(s) if s.to_str().is_ok_and(|s| s == "skins") => {
                    choices.extend(skins.iter().map(|(value, label)| Choice { value: value.clone(), label: label.clone() }));
                    // The default skin leads, the rest follow by folder.
                    let default = text("default")?;
                    if let Some(i) = choices.iter().position(|c| default.as_ref() == Some(&c.value)) {
                        let first = choices.remove(i);
                        choices.insert(0, first);
                    }
                }
                LuaValue::Table(options) => for option in options.sequence_values::<Table>() {
                    let o = option.map_err(|e| format!("{here}: options: {e}"))?;
                    let value: String = o.get(1).map_err(|e| format!("{here}: option value: {e}"))?;
                    let label: Option<String> = o.get(2).map_err(|e| format!("{here}: option label: {e}"))?;
                    choices.push(Choice { label: label.unwrap_or_else(|| value.clone()), value });
                },
                _ => return Err(format!("{here}: options: a list of options or \"skins\"")),
            }
            if choices.is_empty() {
                return Err(format!("{here}: a choice needs options"));
            }
            let default = text("default")?.unwrap_or_else(|| choices[0].value.clone());
            if !choices.iter().any(|c| c.value == default) {
                return Err(format!("{here}: default {default:?} is not an option"));
            }
            defaults.push((key.clone(), Value::Text(default)));
            Kind::Choice { key, options: choices }
        } else if let Some(key) = text("toggle")? {
            let default = t.get::<Option<bool>>("default").map_err(|e| format!("{here}: default: {e}"))?.unwrap_or(false);
            defaults.push((key.clone(), Value::Bool(default)));
            Kind::Toggle { key }
        } else if let Some(key) = text("range")? {
            let (min, max, step) = (number("min")?, number("max")?, number("step")?);
            if !(min < max && step > 0.0) {
                return Err(format!("{here}: a range needs min < max and a positive step"));
            }
            let default = t.get::<Option<f64>>("default").map_err(|e| format!("{here}: default: {e}"))?.unwrap_or(min);
            defaults.push((key.clone(), Value::Number(default.clamp(min, max))));
            Kind::Range { key, min, max, step, unit: text("unit")?.unwrap_or_default() }
        } else if let Some(key) = text("enter")? {
            let boxes = text("boxes")?;
            if boxes.as_deref().is_some_and(|b| b.is_empty() || b.chars().any(|c| !"9A*? ".contains(c))) {
                return Err(format!("{here}: boxes: 9, A, *, ? or spaces"));
            }
            let prompt = text(if boxes.is_some() { "question" } else { "placeholder" })?.unwrap_or_default();
            let max = t.get::<Option<usize>>("max").map_err(|e| format!("{here}: max: {e}"))?.unwrap_or(30);
            Kind::Ask(crate::menu::Ask { key, boxes, prompt, max, run: text("run")? })
        } else if let Some(name) = text("action")? {
            Kind::Action { name }
        } else if let Some(page) = text("info")? {
            let wait = t.get::<Option<f64>>("wait").map_err(|e| format!("{here}: wait: {e}"))?;
            let home = t.get::<Option<bool>>("home").map_err(|e| format!("{here}: home: {e}"))?.unwrap_or(false);
            Kind::Info { text: page, link: text("link")?, button: text("button")?, summary: text("summary")?, wait, home, after: text("after")?,
                         run: text("run")? }
        } else if let Some(command) = text("run")? {
            Kind::Action { name: format!("run:{command}") }
        } else {
            return Err(format!("{here}: needs items, choice, toggle, range, enter, action, run, info, set, heading, back or done"));
        };
        let mut shown = Shown::default();
        for (field, value) in [("when", true), ("unless", false)] {
            if let Some(key) = text(field)? {
                shown.when = Some((key, value));
            }
        }
        shown.busy = text("busy")?;
        shown.intro = text("intro")?;
        shown.tick = text("tick")?;
        shown.title = text("title")?;
        shown.detail = text("detail")?;
        shown.fill = text("fill")?;
        shown.ordered = t.get::<Option<bool>>("ordered").map_err(|e| format!("{here}: ordered: {e}"))?.unwrap_or(false);
        shown.tile = text("tile")?;
        shown.image = text("image")?;
        if let Some(size) = t.get::<Option<Table>>("image_size").map_err(|e| format!("{here}: image_size: {e}"))? {
            let side = |i: usize| size.get::<f32>(i).map_err(|e| format!("{here}: image_size: {e}"));
            shown.image_size = Some((side(1)?, side(2)?));
        }
        // A Set that asks first: its own page, the entry as the answer over Cancel.
        let (kind, style, prompt) = match (kind, text("ask")?) {
            (kind @ Kind::Set { .. }, Some(ask)) => {
                let answer = Item { label: label.clone(), icon: None, style: None, prompt: None, shown: Shown::default(), kind };
                let cancel = Item { label: "Cancel".into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Back };
                (Kind::Menu(vec![answer, cancel]), Some("ask".to_string()), Some(ask))
            }
            (kind, _) => (kind, style, prompt),
        };
        shown.summarize = text("summarize")?;
        if let (Some(key), Some(default), true) = (&shown.summarize, text("default")?, matches!(kind, Kind::Menu(_))) {
            defaults.push((key.clone(), Value::Text(default)));
        }
        if let Some(summaries) = t.get::<Option<Table>>("summaries").map_err(|e| format!("{here}: summaries: {e}"))? {
            for pair in summaries.pairs::<String, String>() {
                shown.summaries.push(pair.map_err(|e| format!("{here}: summaries: {e}"))?);
            }
        }
        if let Some(prompts) = t.get::<Option<Table>>("prompts").map_err(|e| format!("{here}: prompts: {e}"))? {
            for pair in prompts.pairs::<String, String>() {
                shown.prompts.push(pair.map_err(|e| format!("{here}: prompts: {e}"))?);
            }
            // Lua keeps no order: the first by key wins where several apply.
            shown.prompts.sort();
        }
        out.push(Item { label, icon, style, prompt, shown, kind });
    }
    Ok(out)
}

/// What the screen shows when no skin can draw: the target, plainly.
pub fn fallback(painter: &mut dyn Painter, ui: &Ui, error: &str) {
    let d = ui.design;
    painter.fill_circle(d / 2.0, d / 2.0, d / 2.0, Color::BLACK);
    let style = |size: f32| TextStyle {
        font: "fallback".into(),
        size,
        color: Color::WHITE,
        align: Align::Center,
        baseline: Baseline::Middle,
        spacing: 0.0,
        outline: None,
        snap: true,
    };
    let target = if ui.thermostat.unit == "F" { format!("{:.0}", ui.thermostat.target) } else { format!("{:.1}", ui.thermostat.target) };
    painter.text(&target, d / 2.0, d * 0.45, &style(d * 0.25));
    let first_line: String = error.lines().next().unwrap_or("").chars().take(48).collect();
    painter.text("skin error", d / 2.0, d * 0.68, &style(d * 0.05));
    painter.text(&first_line, d / 2.0, d * 0.75, &style(d * 0.03));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::Recorder;

    fn temp_skin(name: &str, skin: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("client-skin-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("skin.lua"), skin).unwrap();
        dir
    }

    #[test]
    fn skins_change_case_in_every_language() {
        let (lua, _) = sandbox(&std::env::temp_dir()).unwrap();
        let upper: String = lua.load(r#"return string.upper("kühlen 制冷") .. ("été"):upper() .. string.lower("ÄÖ")"#).eval().unwrap();
        assert_eq!(upper, "KÜHLEN 制冷ÉTÉäö");
    }

    #[test]
    fn a_font_is_the_first_of_its_files_that_exists() {
        let dir = temp_skin("fallback", "return { name = 't', api = 1, fonts = { a = { 'unit:NoSuchFont.ttf', 'fonts/x.ttf' }, b = { 'unit:Gone.ttf' } }, draw = function() end }");
        std::fs::create_dir_all(dir.join("fonts")).unwrap();
        std::fs::write(dir.join("fonts/x.ttf"), b"").unwrap();
        let skin = Skin::load(&dir).unwrap();
        assert_eq!(skin.fonts, vec![("a".to_string(), dir.join("fonts/x.ttf")), ("b".to_string(), crate::platform::share().join("fonts/Gone.ttf"))]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_layer_is_drawn_once_per_key_and_painted_every_frame() {
        let dir = temp_skin("layer", "return { name = 't', api = 1, draw = function(gfx, ui) \
            gfx:clear('#000') \
            gfx:layer('ring', ui.thermostat.unit, function(g) g:circle(240, 240, 200, '#fff') end) \
            gfx:paint_layer('ring', '#f00', { 0, 0, 480, 0, 240, 480 }) end }");
        let skin = Skin::load(&dir).unwrap();
        let mut rec = Recorder::default();
        skin.draw(&mut rec, &ui()).unwrap();
        skin.draw(&mut rec, &ui()).unwrap();
        assert_eq!(rec.ops, ["clear", "layer ring", "fill_circle 240 240 200", "paint_layer ring 3", "clear", "paint_layer ring 3"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_frame_asks_for_the_next_with_wake() {
        let dir = temp_skin("wake", "return { name = 't', api = 1, draw = function(gfx, ui) \
            if ui.now < 1 then gfx:wake(2.5) gfx:layer('l', 1, function(g) g:wake(1.5) end) end end }");
        let skin = Skin::load(&dir).unwrap();
        assert!(!skin.draw(&mut Recorder::default(), &ui()).unwrap());
        assert_eq!(skin.wake(), Some(Duration::from_secs_f64(1.5)), "the soonest asked, in a layer too");
        skin.draw(&mut Recorder::default(), &Ui { now: 2.0, ..ui() }).unwrap();
        assert_eq!(skin.wake(), None, "only for the frame that asked");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn ui() -> Ui {
        Ui { screen: "home", design: 480.0, face_px: 480.0, thermostat: ThermostatView { target: 21.0, current: 20.5, unit: "C", ..Default::default() }, ..Default::default() }
    }

    #[test]
    fn a_skin_draws_through_gfx_and_requires_its_own_modules() {
        let dir = temp_skin("draws", r##"
            local util = require("util")
            return { name = "t", api = 1, fonts = { regular = "fonts/x.ttf" },
              draw = function(gfx, ui)
                gfx:circle(240, 240, 240, "#ec3100")
                gfx:text(util.fmt(ui.thermostat.target), 240, 240, { font = "regular", size = 100, align = "center" })
                return ui.now < 1
              end }"##);
        std::fs::write(dir.join("util.lua"), "return { fmt = function(t) return string.format('%.0f', t) end }").unwrap();
        let skin = Skin::load(&dir).unwrap();
        assert_eq!(skin.fonts, vec![("regular".to_string(), dir.join("fonts/x.ttf"))]);
        let mut rec = Recorder::default();
        assert!(skin.draw(&mut rec, &ui()).unwrap());
        assert_eq!(rec.ops, vec!["fill_circle 240 240 240".to_string(), "text \"21\" regular".to_string()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn skins_cannot_reach_files_or_the_os() {
        for (name, body) in [
            ("io", "io.open('/etc/passwd')"),
            ("os", "os.execute('true')"),
            ("escape", "require('..x')"),
            ("escape_skin", "require('../x:y')"),
            ("loadfile", "loadfile('/etc/passwd')"),
            ("dofile", "dofile('/etc/passwd')"),
            ("load", "load('return 1')()"),
            ("loadstring", "loadstring('return 1')()"),
            ("dump", "string.dump(function() end)"),
        ] {
            let dir = temp_skin(name, &format!("return {{ name = 't', api = 1, draw = function(gfx, ui) {body} end }}"));
            let skin = Skin::load(&dir).unwrap();
            assert!(skin.draw(&mut Recorder::default(), &ui()).is_err(), "{name}");
            let _ = std::fs::remove_dir_all(dir);
        }
        let dir = temp_skin("font", "return { name = 't', api = 1, fonts = { x = '../x.ttf' }, draw = function() end }");
        assert!(Skin::load(&dir).unwrap_err().contains("outside the skin"));
        let _ = std::fs::remove_dir_all(dir);
        let dir = temp_skin("skinfont", "return { name = 't', api = 1, fonts = { x = { 'other:../x.ttf' } }, draw = function() end }");
        assert!(Skin::load(&dir).unwrap_err().contains("outside the skin"));
        let _ = std::fs::remove_dir_all(dir);
        let dir = temp_skin("unitfont", "return { name = 't', api = 1, fonts = { x = { 'unit:../x.ttf' } }, draw = function() end }");
        assert!(Skin::load(&dir).unwrap_err().contains("not a font name"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_skin_uses_modules_and_fonts_of_the_skins_beside_it() {
        let skins = std::env::temp_dir().join(format!("client-skins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&skins);
        for (dir, skin) in [
            ("base", "return { name = 'Base', api = 1, draw = require('screens') }"),
            ("own", "return { name = 'Own', api = 1, fonts = { x = 'base:fonts/x.ttf' }, draw = require('base:screens') }"),
        ] {
            std::fs::create_dir_all(skins.join(dir).join("fonts")).unwrap();
            std::fs::write(skins.join(dir).join("skin.lua"), skin).unwrap();
            std::fs::write(skins.join(dir).join("look.lua"), format!("return '{dir}'")).unwrap();
        }
        std::fs::write(skins.join("base/fonts/x.ttf"), b"").unwrap();
        // A module of another skin finds that skin's own modules.
        std::fs::write(skins.join("base/screens.lua"),
            "local look = require('look') return function(gfx) gfx:text(look, 0, 0) end").unwrap();
        let skin = Skin::load(&skins.join("own")).unwrap();
        assert_eq!(skin.fonts, vec![("x".to_string(), skins.join("base/fonts/x.ttf"))]);
        let mut rec = Recorder::default();
        skin.draw(&mut rec, &ui()).unwrap();
        assert_eq!(rec.ops, ["text \"base\" regular"]);
        assert_eq!(catalog(&skins), [("base".to_string(), "Base".to_string()), ("own".to_string(), "Own".to_string())]);
        let _ = std::fs::remove_dir_all(skins);
    }

    #[test]
    fn a_choice_can_offer_the_skins() {
        let dir = std::env::temp_dir().join(format!("client-menus-skins-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("menus.lua");
        std::fs::write(&file, r#"return { items = { { label = "Theme", choice = "skin", options = "skins", default = "b" } } }"#).unwrap();
        let skins = [("a".to_string(), "A".to_string()), ("b".to_string(), "B".to_string())];
        let (tree, defaults) = load_menus(&file, &skins).unwrap();
        let Kind::Choice { options, .. } = &tree.items[0].kind else { panic!("not a choice") };
        assert_eq!(options.iter().map(|o| (o.value.as_str(), o.label.as_str())).collect::<Vec<_>>(), [("b", "B"), ("a", "A")], "the default first");
        assert_eq!(defaults, [("skin".to_string(), Value::Text("b".into()))]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_looping_skin_is_stopped_instead_of_hanging_the_ui() {
        let dir = temp_skin("loop", "return { name = 't', api = 1, draw = function() while true do end end }");
        let skin = Skin::load(&dir).unwrap();
        let start = Instant::now();
        assert!(skin.draw(&mut Recorder::default(), &ui()).unwrap_err().contains("longer than"));
        assert!(start.elapsed() < TIME_LIMIT * 4);
        assert!(skin.draw(&mut Recorder::default(), &ui()).is_err(), "and again on the next frame");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn classic_words_the_status_lines_in_stocks_order() {
        let skin = Skin::load(Path::new("skins/classic")).unwrap();
        let texts = |t: ThermostatView| {
            let mut rec = Recorder::default();
            let ui = Ui { screen: "home", design: 480.0, face_px: 480.0,
                          thermostat: ThermostatView { target: 21.0, current: 20.5, unit: "C", mode: "heat".into(), ..t }, ..Default::default() };
            skin.draw(&mut rec, &ui).unwrap();
            rec.ops.into_iter().filter_map(|op| op.strip_prefix("text \"").and_then(|s| s.split('"').next()).map(str::to_string))
                .filter(|s| s.chars().any(char::is_alphabetic)).collect::<Vec<_>>()
        };
        let heating = ThermostatView { hvac: "heating", stage: 1, ..Default::default() };
        let mut t = heating.clone();
        let mut lines = Vec::new();
        // Each step adds what the step before must give way to.
        lines.push(texts(t.clone()));
        t.alt = 1;
        lines.push(texts(t.clone()));
        t.time_to_target = Some(30);
        lines.push(texts(t.clone()));
        t.stage = 2;
        lines.push(texts(t.clone()));
        t.airwave = true;
        lines.push(texts(t.clone()));
        t.drying = true;
        lines.push(texts(t.clone()));
        t.preconditioning = Some("heat");
        lines.push(texts(t.clone()));
        t.aux = true;
        lines.push(texts(t.clone()));
        t.emergency = true;
        lines.push(texts(t.clone()));
        t.press_to = Some("cool");
        lines.push(texts(t.clone()));
        t.safety = true;
        lines.push(texts(t.clone()));
        assert_eq!(lines.iter().map(|l| l.first().cloned().unwrap_or_default()).collect::<Vec<_>>(),
                   ["HEATING", "ALT. HEAT", "IN 35 MIN", "HEAT STAGE 2", "AIRWAVE", "COOL TO DRY", "PREHEATING", "AUX. HEAT", "EMERGENCY HEAT",
                    "PRESS TO COOL", "SAFETY"]);
        let below = |lockout| texts(ThermostatView { starts_in: Some(95), lockout, ..heating.clone() });
        assert!(below(Some("compressor")).contains(&"STARTS IN 1:35".to_string()));
        assert!(below(Some("cycle")).contains(&"DELAYED FOR 1:35".to_string()));
        let drained = texts(ThermostatView { starts_in: Some(95), lockout: Some("compressor"), system_issue: true, ..heating.clone() });
        assert!(drained.contains(&"SYSTEM ISSUE".to_string()) && !drained.iter().any(|s| s.starts_with("STARTS IN")), "{drained:?}");
    }

    #[test]
    fn a_skin_for_another_api_is_refused() {
        let dir = temp_skin("api", "return { name = 't', api = 99, draw = function() end }");
        assert!(Skin::load(&dir).unwrap_err().contains("API 99"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn menus_files_give_the_tree_and_defaults_and_name_their_mistakes() {
        let dir = std::env::temp_dir().join(format!("client-menus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("menus.lua");
        std::fs::write(&file, r#"return { title = "Settings", items = {
            { label = "Mode", choice = "mode", default = "cool", options = { { "heat", "Heat" }, { "cool", "Cool" } } },
            { label = "Display", items = { { label = "Brightness", range = "b", min = 10, max = 100, step = 10, default = 80, unit = "%" } } },
            { label = "Eco", toggle = "eco" },
            { label = "Restart", action = "restart" } } }"#).unwrap();
        let (tree, defaults) = load_menus(&file, &[]).unwrap();
        assert_eq!(tree.items.len(), 4);
        assert_eq!(tree.back, "Back");
        assert_eq!(defaults, vec![("mode".into(), Value::Text("cool".into())), ("b".into(), Value::Number(80.0)), ("eco".into(), Value::Bool(false))]);
        std::fs::write(&file, r#"return { items = { { label = "Display", items = { { label = "Odd" } } } } }"#).unwrap();
        assert_eq!(load_menus(&file, &[]).unwrap_err(), "Settings > Display > Odd: needs items, choice, toggle, range, enter, action, run, info, set, heading, back or done");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn plugins_hide_entries_add_menus_after_the_first_and_read_values() {
        let dir = std::env::temp_dir().join(format!("client-plugins-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(PLUGINS)).unwrap();
        let file = dir.join("menus.lua");
        std::fs::write(&file, r#"return { items = { { label = "Settings", items = {
            { done = true }, { label = "Eco", toggle = "eco" }, { label = "Nest Pro", action = "pro" } } } } }"#).unwrap();
        std::fs::write(dir.join(PLUGINS).join("a.lua"), r#"return { hide = { "nest pro" }, values = { ["x.y"] = "echo hi" },
            menus = { { label = "One", run = "true" }, { label = "Two", info = "{x.y}", image = "run:two.png", wait = 3, run = "true" } } }"#).unwrap();
        let (tree, _) = load_menus(&file, &[]).unwrap();
        let Kind::Menu(settings) = &tree.items[0].kind else { panic!() };
        assert_eq!(settings.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["", "One", "Two", "Eco"]);
        assert_eq!(settings[1].kind, Kind::Action { name: "run:true".into() });
        assert_eq!(settings[2].shown.image.as_deref(), Some("run:two.png"));
        assert_eq!(tree.values, [("x.y".to_string(), "echo hi".to_string())]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn dropped_in_plugins_add_pages_in_name_order_and_replace_the_clients() {
        let dir = std::env::temp_dir().join(format!("client-drop-in-{}", std::process::id()));
        let drop_in = dir.join("drop-in");
        std::fs::create_dir_all(dir.join(PLUGINS)).unwrap();
        std::fs::create_dir_all(&drop_in).unwrap();
        let file = dir.join("menus.lua");
        std::fs::write(&file, r#"return { items = { { label = "Settings", items = { { done = true }, { label = "Eco", toggle = "eco" } } } } }"#).unwrap();
        let page = |label: &str| format!(r#"return {{ menus = {{ {{ label = "{label}", run = "true" }} }} }}"#);
        std::fs::write(dir.join(PLUGINS).join("10-a.lua"), page("A")).unwrap();
        std::fs::write(dir.join(PLUGINS).join("30-c.lua"), page("Bundled C")).unwrap();
        std::fs::write(drop_in.join("20-b.lua"), page("B")).unwrap();
        std::fs::write(drop_in.join("30-c.lua"), page("C")).unwrap();
        std::fs::write(drop_in.join("logo.png"), b"").unwrap();
        let (tree, _) = load_menus_with(&file, &[], &drop_in).unwrap();
        let Kind::Menu(settings) = &tree.items[0].kind else { panic!() };
        assert_eq!(settings.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["", "A", "B", "C", "Eco"]);
        assert_eq!(image_file(&dir, "plugin:logo.png").unwrap(), drop_in.join("logo.png").display().to_string());
        let _ = std::fs::remove_dir_all(dir);
    }
}
