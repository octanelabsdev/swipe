//! `swipe edit`: a full-screen editor for profiles.json. All changes go through `ProfilesFile`,
//! so the editor can't save anything `swipe` itself would refuse to load.

use crate::config::{self, Config};
use crate::profiles_file::{Entry, ProfilesFile, Saved};
use crate::sync;
use anyhow::{Result, bail};
use ratatui::crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};
use std::io::{self, IsTerminal};
use std::path::{MAIN_SEPARATOR, Path};

/// Returns false if a sync run afterwards had problems.
pub fn run() -> Result<bool> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("swipe edit needs a terminal; to change profiles from a script, edit profiles.json");
    }
    let (home, user_home) = config::locate()?;
    let here = tilde(&std::env::current_dir()?, &user_home);
    let mut editor = Editor::new(ProfilesFile::open_in(home, user_home)?, here);

    let mut terminal = ratatui::try_init()?;
    let result = editor.event_loop(&mut terminal);
    ratatui::try_restore()?;
    result?;

    for note in &editor.notes {
        println!("{note}");
    }
    if editor.run_sync {
        return Ok(sync::sync(&Config::load()?, None));
    }
    Ok(true)
}

/// `dir` the way profiles.json usually spells it: under `~` when it's in the home directory.
fn tilde(dir: &Path, user_home: &Path) -> String {
    match dir.strip_prefix(user_home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~{MAIN_SEPARATOR}{}", rest.display()),
        Err(_) => dir.display().to_string(),
    }
}

#[derive(PartialEq)]
enum Focus {
    Profiles,
    Paths,
}

enum Mode {
    Browse,
    Input(Input),
    ConfirmRemove(String),
    ConfirmSync { then_quit: bool },
    Quitting,
}

struct Input {
    label: String,
    note: Option<String>,
    text: String,
    then: Then,
}

enum Then {
    NewName,
    NewConfigDir(String),
    Rename(String),
    ConfigDir(String),
    AddPath(String),
    OpAccount(String),
}

const LOGIN_WARNING: &str =
    "Claude Code may ask you to log in again for this profile. No folder is moved.";

struct Editor {
    file: ProfilesFile,
    /// The folder `swipe edit` started in, offered when adding a path.
    here: String,
    selected: usize,
    path_selected: usize,
    focus: Focus,
    mode: Mode,
    status: Option<(String, bool)>,
    /// Printed after the editor closes.
    notes: Vec<String>,
    run_sync: bool,
}

impl Editor {
    fn new(file: ProfilesFile, here: String) -> Editor {
        Editor {
            file,
            here,
            selected: 0,
            path_selected: 0,
            focus: Focus::Profiles,
            mode: Mode::Browse,
            status: None,
            notes: Vec::new(),
            run_sync: false,
        }
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;
            if let Some(key) = event::read()?.as_key_press_event()
                && self.handle(key)
            {
                return Ok(());
            }
        }
    }

    fn current(&self) -> Option<Entry> {
        self.file.entries().into_iter().nth(self.selected)
    }

    /// Returns true when the editor should close.
    fn handle(&mut self, key: KeyEvent) -> bool {
        let ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        match std::mem::replace(&mut self.mode, Mode::Browse) {
            Mode::Browse if ctrl_c => self.quit(),
            Mode::Browse => self.browse(key.code),
            Mode::Input(input) => {
                self.type_into(input, key);
                false
            }
            Mode::ConfirmRemove(name) => {
                if key.code == KeyCode::Char('y') {
                    let result = self.file.remove_profile(&name);
                    self.report(result, format!("Removed \"{name}\". Press w to save."));
                    self.clamp();
                }
                false
            }
            Mode::ConfirmSync { then_quit } => {
                if key.code == KeyCode::Char('y') {
                    self.run_sync = true;
                    return true;
                }
                then_quit
            }
            Mode::Quitting => match key.code {
                KeyCode::Char('s') | KeyCode::Char('w') => {
                    self.save(true);
                    matches!(self.mode, Mode::Browse) && self.status.as_ref().is_some_and(|s| !s.1)
                }
                KeyCode::Char('d') => true,
                _ => false,
            },
        }
    }

    fn browse(&mut self, code: KeyCode) -> bool {
        let entry = self.current();
        let name = entry.as_ref().map(|e| e.name.clone());
        match (code, name) {
            (KeyCode::Char('q') | KeyCode::Esc, _) => return self.quit(),
            (KeyCode::Up | KeyCode::Char('k'), _) => self.step(-1),
            (KeyCode::Down | KeyCode::Char('j'), _) => self.step(1),
            (KeyCode::Tab | KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l'), _) => {
                self.focus = match self.focus {
                    Focus::Profiles if entry.is_some_and(|e| !e.paths.is_empty()) => Focus::Paths,
                    _ => Focus::Profiles,
                };
                self.path_selected = 0;
            }
            (KeyCode::Char('a'), _) => self.ask("New profile name", None, "", Then::NewName),
            (KeyCode::Char('w'), _) => self.save(false),
            (_, None) => {}
            (KeyCode::Char('r'), Some(name)) => {
                self.ask("Rename to", None, &name.clone(), Then::Rename(name))
            }
            (KeyCode::Char('c'), Some(name)) => {
                let dir = entry.map(|e| e.config_dir).unwrap_or_default();
                self.ask(
                    "Config dir",
                    Some(LOGIN_WARNING),
                    &dir,
                    Then::ConfigDir(name),
                )
            }
            (KeyCode::Char('p'), Some(name)) => {
                let here = self.here.clone();
                self.ask("Add path", None, &here, Then::AddPath(name))
            }
            (KeyCode::Char('o'), Some(name)) => {
                let account = entry.and_then(|e| e.op_account).unwrap_or_default();
                let note = Some("Leave empty to remove it. `op account list` shows yours.");
                self.ask("1Password account", note, &account, Then::OpAccount(name))
            }
            (KeyCode::Char('s'), Some(name)) => {
                let clear = self.file.default() == Some(name.as_str());
                let result = self.file.set_default((!clear).then_some(name.as_str()));
                let message = if clear {
                    "No default profile now.".into()
                } else {
                    format!("\"{name}\" is now the default.")
                };
                self.report(result, message);
            }
            (KeyCode::Char('d') | KeyCode::Delete, Some(name)) => {
                if self.focus == Focus::Paths {
                    let path = entry.and_then(|e| e.paths.into_iter().nth(self.path_selected));
                    if let Some(path) = path {
                        let result = self.file.remove_path(&name, &path);
                        self.report(result, format!("Removed {path} from \"{name}\"."));
                        self.clamp();
                    }
                } else {
                    self.mode = Mode::ConfirmRemove(name);
                }
            }
            _ => {}
        }
        false
    }

    fn type_into(&mut self, mut input: Input, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Enter => self.submit(input),
            KeyCode::Backspace => {
                input.text.pop();
                self.mode = Mode::Input(input);
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                input.text.push(c);
                self.mode = Mode::Input(input);
            }
            _ => self.mode = Mode::Input(input),
        }
    }

    fn submit(&mut self, input: Input) {
        let text = input.text.trim().to_string();
        match input.then {
            Then::NewName => {
                if self.file.entries().iter().any(|e| e.name == text) {
                    self.status =
                        Some((format!("a profile named \"{text}\" already exists"), true));
                    return;
                }
                let dir = format!("~{MAIN_SEPARATOR}.claude-{text}");
                self.ask("Config dir", None, &dir, Then::NewConfigDir(text));
            }
            Then::NewConfigDir(name) => {
                let result = self.file.add_profile(&name, &text);
                if result.is_ok() {
                    self.selected = self.file.entries().len() - 1;
                    self.focus = Focus::Profiles;
                }
                self.report(
                    result,
                    format!("Added \"{name}\". Press p to give it a path."),
                );
            }
            Then::Rename(name) => {
                let result = self.file.rename_profile(&name, &text);
                self.report(result, format!("Renamed \"{name}\" to \"{text}\"."));
            }
            Then::ConfigDir(name) => {
                let result = self.file.set_config_dir(&name, &text);
                self.report(result, format!("Changed the config dir. {LOGIN_WARNING}"));
            }
            Then::AddPath(name) => {
                let result = self.file.add_path(&name, &text);
                self.report(result, format!("Added {text} to \"{name}\"."));
            }
            Then::OpAccount(name) => {
                let account = (!text.is_empty()).then_some(text.as_str());
                let result = self.file.set_op_account(&name, account);
                self.report(result, format!("Set the 1Password account for \"{name}\"."));
            }
        }
    }

    fn save(&mut self, then_quit: bool) {
        match self.file.save() {
            Ok(saved) => self.saved(saved, then_quit),
            Err(e) => self.status = Some((format!("{e:#}"), true)),
        }
    }

    fn saved(&mut self, saved: Saved, then_quit: bool) {
        for (name, config_dir, source) in saved.removed {
            self.notes.push(format!(
                "swipe: removed \"{name}\"; its config dir {} and source folder {} are still on disk",
                config_dir.display(),
                source.display()
            ));
        }
        self.status = Some((format!("Saved {}.", self.file.path().display()), false));
        if saved.needs_sync {
            self.mode = Mode::ConfirmSync { then_quit };
        }
    }

    fn quit(&mut self) -> bool {
        if self.file.is_changed() {
            self.mode = Mode::Quitting;
            false
        } else {
            true
        }
    }

    fn ask(&mut self, label: &str, note: Option<&str>, text: &str, then: Then) {
        self.mode = Mode::Input(Input {
            label: label.into(),
            note: note.map(String::from),
            text: text.into(),
            then,
        });
    }

    fn report(&mut self, result: Result<()>, success: String) {
        self.status = Some(match result {
            Ok(()) => (success, false),
            Err(e) => (format!("{e:#}"), true),
        });
    }

    fn step(&mut self, by: isize) {
        let len = match self.focus {
            Focus::Profiles => self.file.entries().len(),
            Focus::Paths => self.current().map_or(0, |e| e.paths.len()),
        };
        let index = match self.focus {
            Focus::Profiles => &mut self.selected,
            Focus::Paths => &mut self.path_selected,
        };
        if len > 0 {
            *index = index.saturating_add_signed(by).min(len - 1);
        }
        if self.focus == Focus::Profiles {
            self.path_selected = 0;
        }
    }

    /// Keeps the selection on something that exists after a removal.
    fn clamp(&mut self) {
        let len = self.file.entries().len();
        self.selected = self.selected.min(len.saturating_sub(1));
        let paths = self.current().map_or(0, |e| e.paths.len());
        if paths == 0 {
            self.focus = Focus::Profiles;
        }
        self.path_selected = self.path_selected.min(paths.saturating_sub(1));
    }

    fn draw(&self, frame: &mut Frame) {
        let entries = self.file.entries();
        let [profiles_area, paths_area, status_area, help_area] = Layout::vertical([
            Constraint::Min(4),
            Constraint::Length(7),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .areas(frame.area());

        let width = entries.iter().map(|e| e.name.len()).max().unwrap_or(0);
        let items: Vec<ListItem> = entries
            .iter()
            .map(|e| {
                let mut spans = vec![
                    Span::styled(format!("{:width$}", e.name), Style::new().bold()),
                    Span::raw(format!("  {}", e.config_dir)),
                ];
                if let Some(account) = &e.op_account {
                    spans.push(Span::styled(
                        format!("  1Password: {account}"),
                        Style::new().fg(Color::Cyan),
                    ));
                }
                if self.file.default() == Some(e.name.as_str()) {
                    spans.push(Span::styled("  (default)", Style::new().fg(Color::Green)));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let empty = entries.is_empty();
        let unsaved = if self.file.is_changed() {
            " (unsaved)"
        } else {
            ""
        };
        // The marker goes before the path, so a long path is what gets cut off.
        let title = format!(" swipe profiles{unsaved}: {} ", self.file.path().display());
        let list = if empty {
            List::new([ListItem::new("No profiles yet. Press a to add one.")])
        } else {
            List::new(items)
        };
        let mut state = ListState::default().with_selected((!empty).then_some(self.selected));
        frame.render_stateful_widget(
            list.block(Block::bordered().title(title))
                .highlight_symbol("> ")
                .highlight_style(self.highlight(Focus::Profiles)),
            profiles_area,
            &mut state,
        );

        let current = entries.get(self.selected);
        let paths: Vec<ListItem> = match current {
            Some(e) if !e.paths.is_empty() => {
                e.paths.iter().map(|p| ListItem::new(p.as_str())).collect()
            }
            Some(_) => vec![ListItem::new(
                "No paths: used only as the default. Press p to add one.",
            )],
            None => Vec::new(),
        };
        let title = current.map_or(" Paths ".into(), |e| format!(" Paths for {} ", e.name));
        let mut state = ListState::default()
            .with_selected((self.focus == Focus::Paths).then_some(self.path_selected));
        frame.render_stateful_widget(
            List::new(paths)
                .block(Block::bordered().title(title))
                .highlight_symbol("> ")
                .highlight_style(self.highlight(Focus::Paths)),
            paths_area,
            &mut state,
        );

        let red = Style::new().fg(Color::Red);
        let status = match (&self.status, self.file.problem()) {
            (Some((message, true)), _) => Line::styled(message.clone(), red),
            (_, Some(problem)) => Line::styled(format!("Can't save yet: {problem}"), red),
            (Some((message, false)), None) => Line::raw(message.clone()),
            (None, None) if self.file.is_changed() => {
                Line::raw("Unsaved changes. Press w to save.")
            }
            (None, None) => Line::raw(""),
        };
        frame.render_widget(
            Paragraph::new(status).wrap(Wrap { trim: false }),
            status_area,
        );

        let help: Vec<Line> = match &self.mode {
            Mode::Browse => vec![
                Line::raw(
                    "[a]dd  [r]ename  [d]elete  [s]et default  [c]onfig dir  [p]ath  [o] 1Password",
                ),
                Line::raw("[↑↓] move  [tab] profiles/paths  [w] save  [q]uit"),
            ],
            Mode::Input(input) => {
                let line = Line::from(vec![
                    Span::styled(format!("{}: ", input.label), Style::new().bold()),
                    Span::raw(input.text.clone()),
                ]);
                let note = input
                    .note
                    .clone()
                    .unwrap_or_else(|| "[enter] ok  [esc] cancel".into());
                let x = help_area.x + (input.label.len() + 2 + input.text.chars().count()) as u16;
                frame.set_cursor_position(Position::new(x, help_area.y));
                vec![line, Line::raw(note)]
            }
            Mode::ConfirmRemove(name) => vec![
                Line::raw(format!("Remove \"{name}\" from profiles.json? [y/N]")),
                Line::raw("Its config dir and source folder stay on disk."),
            ],
            Mode::ConfirmSync { .. } => vec![
                Line::raw("Run swipe sync now? [y/N]"),
                Line::raw(
                    "This closes the editor and links each profile's files into its config dir.",
                ),
            ],
            Mode::Quitting => vec![
                Line::raw("You have unsaved changes."),
                Line::raw("[s]ave and quit  [d]iscard and quit  [esc] go back"),
            ],
        };
        frame.render_widget(Paragraph::new(help), help_area);
    }

    fn highlight(&self, focus: Focus) -> Style {
        if self.focus == focus {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::fs;
    use std::path::PathBuf;

    const FILE: &str = r#"{
  "default": "personal",
  "profiles": {
    "personal": { "config_dir": "~/.claude" },
    "work": { "config_dir": "~/.claude-work", "paths": ["~/code/acme"], "op_account": "acme" }
  }
}
"#;

    struct Setup {
        _dir: tempfile::TempDir,
        home: PathBuf,
        user_home: PathBuf,
        editor: Editor,
    }

    impl Setup {
        fn new(contents: Option<&str>) -> Setup {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("swipe");
            let user_home = dir.path().join("user");
            fs::create_dir_all(user_home.join("code/acme")).unwrap();
            fs::create_dir_all(user_home.join("code/client")).unwrap();
            if let Some(contents) = contents {
                fs::create_dir_all(&home).unwrap();
                fs::write(home.join("profiles.json"), contents).unwrap();
            }
            let file = ProfilesFile::open_in(home.clone(), user_home.clone()).unwrap();
            Setup {
                _dir: dir,
                home,
                user_home,
                editor: Editor::new(file, "~/code/client".into()),
            }
        }

        /// Presses each key in turn; returns true if the editor asked to close.
        fn keys(&mut self, keys: &[KeyCode]) -> bool {
            let mut closed = false;
            for &code in keys {
                closed = self.editor.handle(KeyEvent::from(code));
            }
            closed
        }

        fn key(&mut self, c: char) -> bool {
            self.keys(&[KeyCode::Char(c)])
        }

        /// Replaces whatever the open prompt holds with `text` and presses enter.
        fn answer(&mut self, text: &str) {
            let Mode::Input(input) = &self.editor.mode else {
                panic!("no prompt open:\n{}", self.screen());
            };
            let len = input.text.chars().count();
            self.keys(&vec![KeyCode::Backspace; len]);
            let typed: Vec<KeyCode> = text.chars().map(KeyCode::Char).collect();
            self.keys(&typed);
            self.keys(&[KeyCode::Enter]);
        }

        fn screen(&self) -> String {
            let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
            terminal.draw(|f| self.editor.draw(f)).unwrap();
            terminal.backend().to_string()
        }

        fn text(&self) -> String {
            fs::read_to_string(self.home.join("profiles.json")).unwrap()
        }

        fn which(&self, dir: &str) -> (String, Option<String>) {
            let config = Config::load_from(self.home.clone(), self.user_home.clone()).unwrap();
            let p = config.profile_for(&self.user_home.join(dir)).unwrap();
            (p.name.clone(), p.op_account.clone())
        }

        fn select(&mut self, name: &str) {
            let index = self
                .editor
                .file
                .entries()
                .iter()
                .position(|e| e.name == name);
            self.editor.selected = index.unwrap();
        }
    }

    fn assert_shows(screen: &str, text: &str) {
        assert!(
            screen.contains(text),
            "screen should show {text:?}:\n{screen}"
        );
    }

    #[test]
    fn the_editor_lists_profiles_with_their_dirs_accounts_and_the_default() {
        let s = Setup::new(Some(FILE));
        let screen = s.screen();
        assert_shows(&screen, "personal  ~/.claude");
        assert_shows(&screen, "(default)");
        assert_shows(&screen, "work      ~/.claude-work  1Password: acme");
        assert_shows(&screen, "[a]dd");
    }

    #[test]
    fn adding_a_profile_with_a_path_and_account_then_saving_makes_swipe_use_it() {
        let mut s = Setup::new(Some(FILE));
        s.key('a');
        s.answer("client");
        assert_shows(
            &s.screen(),
            &format!("Config dir: ~{MAIN_SEPARATOR}.claude-client"),
        );
        s.answer("~/.claude-client");
        s.key('p');
        assert_shows(&s.screen(), "Add path: ~/code/client");
        s.answer("~/code/client");
        s.key('o');
        s.answer("client.1password.com");
        assert_shows(&s.screen(), "swipe profiles (unsaved): ");
        assert_eq!(s.text(), FILE, "nothing is written before saving");

        s.key('w');
        assert_shows(&s.screen(), "Run swipe sync now? [y/N]");
        assert!(!s.key('n'));
        assert!(!s.screen().contains("(unsaved)"), "{}", s.screen());
        assert_eq!(
            s.which("code/client"),
            ("client".into(), Some("client.1password.com".into()))
        );
        let text = s.text();
        assert!(
            text.find("\"work\": {") < text.find("\"client\": {"),
            "{text}"
        );
    }

    #[test]
    fn a_change_swipe_would_refuse_to_load_is_shown_and_not_saved() {
        let mut s = Setup::new(Some(FILE));
        s.key('a');
        s.answer("copy");
        s.answer("~/.claude-work");
        assert_shows(&s.screen(), "Can't save yet");
        assert_shows(&s.screen(), "share the config_dir");
        s.key('w');
        assert_shows(&s.screen(), "not saved");
        assert_eq!(s.text(), FILE);
    }

    #[test]
    fn quitting_with_unsaved_changes_asks_and_discard_leaves_the_file_alone() {
        let mut s = Setup::new(Some(FILE));
        s.key('s');
        assert!(!s.key('q'));
        assert_shows(&s.screen(), "[s]ave and quit  [d]iscard and quit");
        assert!(!s.keys(&[KeyCode::Esc]), "esc goes back");
        assert!(!s.key('q'));
        assert!(s.key('d'));
        assert_eq!(s.text(), FILE);
    }

    #[test]
    fn quitting_with_unsaved_changes_can_save_on_the_way_out() {
        let mut s = Setup::new(Some(FILE));
        s.editor.selected = 1;
        s.key('s');
        s.key('q');
        assert!(s.key('s'));
        assert_eq!(s.which("code/client").0, "work", "work is the default now");
    }

    #[test]
    fn quitting_without_changes_closes_at_once() {
        let mut s = Setup::new(Some(FILE));
        assert!(s.key('q'));
    }

    #[test]
    fn the_first_profile_added_to_a_new_setup_becomes_the_default() {
        let mut s = Setup::new(None);
        assert_shows(&s.screen(), "No profiles yet. Press a to add one.");
        s.key('a');
        s.answer("personal");
        s.answer("~/.claude");
        s.key('w');
        s.key('n');
        assert_eq!(s.which("code/acme").0, "personal");
        assert_shows(&s.screen(), "(default)");
    }

    #[test]
    fn removing_a_profile_keeps_its_folders_and_reports_them_after_closing() {
        let mut s = Setup::new(Some(FILE));
        fs::create_dir_all(s.user_home.join(".claude-work")).unwrap();
        fs::create_dir_all(s.home.join("profiles/work")).unwrap();
        s.select("work");
        s.key('d');
        assert_shows(&s.screen(), "Remove \"work\" from profiles.json? [y/N]");
        s.key('y');
        s.key('w');
        assert!(!s.text().contains("work"), "{}", s.text());
        assert!(s.user_home.join(".claude-work").is_dir());
        assert!(s.home.join("profiles/work").is_dir());
        let note = &s.editor.notes[0];
        assert!(
            note.contains(".claude-work") && note.contains("profiles"),
            "{note}"
        );
    }

    #[test]
    fn removing_the_default_profile_shows_there_is_no_default() {
        let mut s = Setup::new(Some(FILE));
        s.select("personal");
        s.key('d');
        s.key('y');
        assert!(!s.screen().contains("(default)"), "{}", s.screen());
        s.key('w');
        assert!(!s.text().contains("\"default\""), "{}", s.text());
    }

    #[test]
    fn renaming_a_profile_moves_its_source_folder_on_save() {
        let mut s = Setup::new(Some(FILE));
        fs::create_dir_all(s.home.join("profiles/work")).unwrap();
        s.select("work");
        s.key('r');
        s.answer("job");
        s.key('w');
        assert_shows(&s.screen(), "Run swipe sync now?");
        s.key('n');
        assert!(s.home.join("profiles/job").is_dir());
        assert_eq!(s.which("code/acme").0, "job");
    }

    #[test]
    fn renaming_onto_an_existing_source_folder_is_refused() {
        let mut s = Setup::new(Some(FILE));
        fs::create_dir_all(s.home.join("profiles/job")).unwrap();
        s.select("work");
        s.key('r');
        s.answer("job");
        assert_shows(&s.screen(), "already exists");
        assert!(!s.editor.file.is_changed());
    }

    #[test]
    fn changing_a_config_dir_warns_about_logging_in_before_saving() {
        let mut s = Setup::new(Some(FILE));
        s.select("work");
        s.key('c');
        assert_shows(&s.screen(), "may ask you to log in again");
        s.answer("~/.claude-job");
        assert_shows(&s.screen(), "may ask you to log in again");
        assert!(!s.user_home.join(".claude-job").exists());
    }

    #[test]
    fn saying_yes_to_sync_closes_the_editor_to_run_it() {
        let mut s = Setup::new(Some(FILE));
        s.select("work");
        s.key('c');
        s.answer("~/.claude-job");
        s.key('w');
        assert!(s.key('y'));
        assert!(s.editor.run_sync);
    }

    #[test]
    fn path_account_and_default_changes_save_without_asking_to_sync() {
        let mut s = Setup::new(Some(FILE));
        s.select("work");
        s.keys(&[KeyCode::Tab]);
        s.key('d');
        s.key('p');
        s.answer("~/code/client");
        s.key('o');
        s.answer("");
        s.key('s');
        s.key('w');
        assert!(matches!(s.editor.mode, Mode::Browse), "{}", s.screen());
        assert_eq!(s.which("code/client"), ("work".into(), None));
        assert_eq!(s.which("code/acme"), ("work".into(), None));
        assert!(!s.text().contains("acme"), "{}", s.text());
    }

    #[test]
    fn escape_cancels_a_prompt_without_changing_anything() {
        let mut s = Setup::new(Some(FILE));
        s.key('r');
        s.keys(&[KeyCode::Char('x'), KeyCode::Esc]);
        assert!(matches!(s.editor.mode, Mode::Browse));
        assert!(!s.editor.file.is_changed());
    }
}
