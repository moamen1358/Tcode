//! Mission Control (Alt+M): a floating board showing what each agent in the session
//! is doing — who's editing which file, how busy they are, and which files two
//! agents are both touching. It reads the Conductor's per-session ledger
//! (`<bus_dir>/events.jsonl`) and refreshes live via a `gio::FileMonitor`.
//!
//! The aggregation is pure and unit-tested in `tcode_core::conductor::parse_board`;
//! this module only renders that summary into widgets and watches the file. It owns
//! no agent logic, so a future event kind (reads, runs, delegations) flows through
//! by extending the core parser, not this view.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, Align, Box as GtkBox, Label, Orientation, PolicyType, ScrolledWindow};

use tcode_core::conductor::{parse_board, AgentStatus};

/// A floating board (registered with the `OverlayHost`, toggled by Alt+M) over the
/// active session. Cloneable (Rc-backed) so it lives in both `State` and
/// `LiveContent` alongside the other per-session panels, and so the file watcher can
/// hold a clone to re-render on each ledger change.
#[derive(Clone)]
pub struct MissionControl {
    /// The card widget added to the `OverlayHost`.
    pub root: GtkBox,
    inner: Rc<RefCell<Inner>>,
}

struct Inner {
    /// Subtitle under the title: agent + conflict counts, or a hint when idle/off.
    status: Label,
    /// The list we clear and repopulate on each render (agent rows, then conflicts).
    body: GtkBox,
    /// The ledger this board reflects (`<bus_dir>/events.jsonl`), or `None` when
    /// coordination is off / this session has no bus.
    log: Option<PathBuf>,
    /// Live monitor on the ledger; kept alive so edits re-render (a dropped monitor
    /// stops delivering). Replaced whenever `watch` re-points the board.
    monitor: Option<gio::FileMonitor>,
}

impl MissionControl {
    /// Build an empty board card. Call [`watch`](Self::watch) to point it at a
    /// session's ledger and arm the live refresh.
    pub fn new() -> MissionControl {
        let root = GtkBox::new(Orientation::Vertical, 0);
        root.add_css_class("mission-control");
        root.set_size_request(440, -1);

        let title = Label::new(Some("Mission Control"));
        title.add_css_class("mc-title");
        title.set_halign(Align::Start);
        root.append(&title);

        let status = Label::new(None);
        status.add_css_class("mc-status");
        status.set_halign(Align::Start);
        status.set_wrap(true);
        status.set_xalign(0.0);
        root.append(&status);

        let body = GtkBox::new(Orientation::Vertical, 0);
        body.add_css_class("mc-body");
        let scroll = ScrolledWindow::new();
        scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
        scroll.set_min_content_height(96);
        scroll.set_max_content_height(440);
        scroll.set_propagate_natural_height(true);
        scroll.set_child(Some(&body));
        root.append(&scroll);

        let mc = MissionControl {
            root,
            inner: Rc::new(RefCell::new(Inner {
                status,
                body,
                log: None,
                monitor: None,
            })),
        };
        mc.render(); // initial (idle) state until watch() points it at a ledger
        mc
    }

    /// Point the board at `bus_dir`'s ledger (or clear it when `None`), render once,
    /// and arm a live `FileMonitor` so later edits refresh it. Replaces any previous
    /// watch, so it's safe to call on each session (re)build.
    pub fn watch(&self, bus_dir: Option<PathBuf>) {
        let log = bus_dir.map(|d| d.join("events.jsonl"));
        {
            let mut inner = self.inner.borrow_mut();
            inner.monitor = None; // drop the old monitor before re-pointing
            inner.log = log.clone();
        }
        if let Some(log) = log {
            // Watch the file even before it exists: the first hook write fires Created,
            // and each append fires Changed/ChangesDoneHint — all re-render the board.
            let file = gio::File::for_path(&log);
            if let Ok(monitor) =
                file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            {
                let this = self.clone();
                monitor.connect_changed(move |_m, _file, _other, event| {
                    use gio::FileMonitorEvent as E;
                    if matches!(event, E::Changed | E::ChangesDoneHint | E::Created) {
                        this.render();
                    }
                });
                self.inner.borrow_mut().monitor = Some(monitor);
            }
        }
        self.render();
    }

    /// Re-read the ledger and rebuild the rows. Cheap — a file read plus a handful of
    /// widget swaps, driven by a low-frequency file watch.
    fn render(&self) {
        let inner = self.inner.borrow();
        let board = inner
            .log
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| parse_board(&s))
            .unwrap_or_default();

        while let Some(child) = inner.body.first_child() {
            inner.body.remove(&child);
        }

        if inner.log.is_none() {
            inner
                .status
                .set_text("Coordination isn't active for this session.");
            return;
        }
        if board.agents.is_empty() {
            inner.status.set_text("Waiting for the agents' first edits…");
            return;
        }

        let n = board.agents.len();
        let summary = match board.conflicts.len() {
            0 => format!("{n} agent{} · no conflicts", plural(n)),
            c => format!("{n} agent{} · ⚠ {c} conflict{}", plural(n), plural(c)),
        };
        inner.status.set_text(&summary);

        for a in &board.agents {
            inner.body.append(&agent_row(a));
        }
        if !board.conflicts.is_empty() {
            let header = Label::new(Some("Same file, two agents"));
            header.add_css_class("mc-section");
            header.set_halign(Align::Start);
            inner.body.append(&header);
            for c in &board.conflicts {
                let row = Label::new(Some(&format!("⚠  {}  ({})", c.base, c.agents.join(", "))));
                row.add_css_class("mc-conflict");
                row.set_halign(Align::Start);
                row.set_wrap(true);
                row.set_xalign(0.0);
                inner.body.append(&row);
            }
        }
    }
}

impl Default for MissionControl {
    fn default() -> Self {
        Self::new()
    }
}

/// One agent's row: a kind-colored dot, its id, the file it's on, and its edit count.
fn agent_row(a: &AgentStatus) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.add_css_class("mc-agent");

    let dot = Label::new(Some("●"));
    dot.add_css_class("mc-dot");
    dot.add_css_class(agent_kind_class(&a.agent));
    dot.set_valign(Align::Start);
    row.append(&dot);

    let col = GtkBox::new(Orientation::Vertical, 1);
    col.set_hexpand(true);
    let name = Label::new(Some(&a.agent));
    name.add_css_class("mc-agent-id");
    name.set_halign(Align::Start);
    col.append(&name);
    let detail = if a.last_base.is_empty() {
        "idle".to_string()
    } else {
        format!("editing {}", a.last_base)
    };
    let sub = Label::new(Some(&detail));
    sub.add_css_class("mc-agent-file");
    sub.set_halign(Align::Start);
    sub.set_wrap(true);
    sub.set_xalign(0.0);
    col.append(&sub);
    row.append(&col);

    let count = Label::new(Some(&format!("{} edit{}", a.edits, plural(a.edits))));
    count.add_css_class("mc-count");
    count.set_valign(Align::Center);
    row.append(&count);

    row
}

/// The dot's CSS class, chosen by the agent id's kind prefix (claude/codex/hermes).
fn agent_kind_class(agent: &str) -> &'static str {
    if agent.starts_with("codex") {
        "kind-codex"
    } else if agent.starts_with("hermes") {
        "kind-hermes"
    } else {
        "kind-claude"
    }
}

/// English plural suffix for a count (`""` for 1, else `"s"`).
fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
