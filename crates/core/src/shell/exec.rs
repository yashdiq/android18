//! REPL implementation: parsing, command dispatch, and output lines.

use crate::domain::Device;
use crate::fs::paths::{STORAGE_ROOT, display_path, resolve};
use crate::port::{DeviceBackend, SearchProvider};
use crate::util::categorize::FileCategory;
use crate::util::format::{format_bytes, format_date};

/// Styling class of a terminal line (drives color choice in the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// The echoed command with prompt.
    Cmd,
    /// Plain output.
    Output,
    /// Error text.
    Error,
    /// AI search result lines.
    Ai,
    /// Success confirmation.
    Success,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellLine {
    pub kind: OutputKind,
    pub text: String,
}

impl ShellLine {
    /// Builds one styled line (used by the UI to echo commands).
    pub fn new(kind: OutputKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

/// The full reply to one executed command line.
#[derive(Debug, Default, PartialEq)]
pub struct ShellReply {
    pub lines: Vec<ShellLine>,
    /// `true` for `clear`: the UI should wipe the transcript.
    pub clear: bool,
    /// `true` when the command changed device state (mkdir/touch/rm/cp/mv):
    /// the UI refreshes its listing and tree snapshot.
    pub mutated: bool,
}

/// A terminal session bound to one device: current directory + history.
pub struct ShellSession {
    cwd: String,
    history: Vec<String>,
    host: String,
}

impl ShellSession {
    /// `device_model` becomes the shell prompt host (e.g. `pixel-9-pro`).
    pub fn new(device_model: &str) -> Self {
        Self {
            cwd: STORAGE_ROOT.to_string(),
            history: Vec::new(),
            host: device_model.to_lowercase().replace(' ', "-"),
        }
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Prompt string, e.g. `android18@pixel-9-pro:~/DCIM$ `.
    pub fn prompt(&self) -> String {
        format!("android18@{}:{}$ ", self.host, display_path(&self.cwd))
    }

    /// Banner shown when the terminal opens.
    pub fn welcome(&self, device: &Device) -> Vec<ShellLine> {
        vec![
            ShellLine::new(
                OutputKind::Output,
                "Android18 Terminal Client v0.1.0 [Phone-as-Server Shell]",
            ),
            ShellLine::new(
                OutputKind::Output,
                format!(
                    "Connected to {} via {} (port {})",
                    device.name,
                    device.transport.label().to_uppercase(),
                    device.port
                ),
            ),
            ShellLine::new(
                OutputKind::Output,
                "Type \"help\" for a list of available commands or \"ai <query>\" for AI search.",
            ),
        ]
    }

    /// Resolves `target` relative to the session cwd, then canonicalizes.
    fn abs(&self, target: &str) -> Result<String, crate::domain::DeviceError> {
        let joined = if target.starts_with('/') {
            target.to_string()
        } else {
            format!("{}/{}", self.cwd.trim_end_matches('/'), target)
        };
        resolve(&joined)
    }
}

impl ShellSession {
    /// Executes one input line against `backend` and produces the reply.
    pub async fn execute(
        &mut self,
        line: &str,
        backend: &dyn DeviceBackend,
        token: &str,
        search: &dyn SearchProvider,
    ) -> ShellReply {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return ShellReply::default();
        }
        self.history.push(trimmed.to_string());

        let mut words = trimmed.split_whitespace();
        let Some(name) = words.next() else {
            return ShellReply::default();
        };
        let args: Vec<&str> = words.collect();

        let mut reply = ShellReply::default();
        reply.lines.push(ShellLine::new(
            OutputKind::Cmd,
            format!("{}{}", self.prompt(), trimmed),
        ));

        match name.to_lowercase().as_str() {
            "help" => Self::cmd_help(&mut reply),
            "pwd" => reply
                .lines
                .push(ShellLine::new(OutputKind::Output, self.cwd.clone())),
            "clear" | "cls" => reply.clear = true,
            "ls" | "dir" => self.cmd_ls(&args, backend, token, &mut reply).await,
            "ll" => self.cmd_ls(&["-l"], backend, token, &mut reply).await,
            "cd" => self.cmd_cd(&args, backend, token, &mut reply).await,
            "cat" => self.cmd_cat(&args, backend, token, &mut reply).await,
            "mkdir" => {
                reply.mutated = self.cmd_mkdir(&args, backend, token, &mut reply).await;
            }
            "touch" => {
                reply.mutated = self.cmd_touch(&args, backend, token, &mut reply).await;
            }
            "rm" => {
                reply.mutated = self.cmd_rm(&args, backend, token, &mut reply).await;
            }
            "cp" => {
                reply.mutated = self.cmd_cp(&args, backend, token, &mut reply).await;
            }
            "mv" => {
                reply.mutated = self.cmd_mv(&args, backend, token, &mut reply).await;
            }
            "stat" => self.cmd_stat(&args, backend, token, &mut reply).await,
            "search" => {
                self.cmd_search(&args, backend, token, search, &mut reply)
                    .await
            }
            "tree" => self.cmd_tree(backend, token, &mut reply).await,
            "ai" => self.cmd_ai(&args, backend, token, search, &mut reply).await,
            other => reply.lines.push(ShellLine::new(
                OutputKind::Error,
                format!("android18: command not found: {other}. Type 'help' for commands."),
            )),
        }
        reply
    }

    fn cmd_help(reply: &mut ShellReply) {
        let lines = [
            "Available commands:",
            "  help                 Show this help message",
            "  ls [-l] [path]       List directory contents (long format with -l)",
            "  cd <path>            Change directory (supports ..)",
            "  pwd                  Print working directory",
            "  cat <file>           Print file contents",
            "  mkdir <name>         Create a directory",
            "  touch <name>         Create an empty file",
            "  rm <path>            Remove file/directory (recursive)",
            "  cp <src> <dest>      Copy a file/folder into a destination folder",
            "  mv <src> <dest>      Move into a folder, or onto a new full path",
            "  stat <path>          Show size, type and modified time",
            "  search <query>       Filename/path search over the whole tree",
            "  tree                 Show directory contents as a tree",
            "  ai <query>           AI semantic search (e.g. \"ai camera photos\")",
            "  clear                Clear the terminal screen",
            "",
            "Aliases: dir = ls, ll = ls -l, cls = clear",
        ];
        for l in lines {
            reply.lines.push(ShellLine::new(OutputKind::Output, l));
        }
    }

    async fn cmd_ls(
        &self,
        args: &[&str],
        backend: &dyn DeviceBackend,
        token: &str,
        reply: &mut ShellReply,
    ) {
        let long = args.contains(&"-l");
        let target = args.iter().find(|a| !a.starts_with('-')).copied();
        let path = match target {
            Some(t) => match self.abs(t) {
                Ok(p) => p,
                Err(e) => {
                    reply
                        .lines
                        .push(ShellLine::new(OutputKind::Error, format!("ls: {e}")));
                    return;
                }
            },
            None => self.cwd.clone(),
        };
        match backend.list(&path, token).await {
            Ok(entries) => {
                if long {
                    reply.lines.push(ShellLine::new(
                        OutputKind::Output,
                        format!("total {}", entries.len()),
                    ));
                    for e in &entries {
                        let perm = if e.dir { "drwxr-xr-x" } else { "-rw-r--r--" };
                        let extra = match (e.dir, e.item_count) {
                            (true, Some(n)) => format!(" ({n} items)"),
                            _ => String::new(),
                        };
                        reply.lines.push(ShellLine::new(
                            OutputKind::Output,
                            format!(
                                "{perm} user0 user0 {:>8} {}{extra}",
                                format_bytes(e.size, 0),
                                e.name
                            ),
                        ));
                    }
                } else {
                    reply.lines.push(ShellLine::new(
                        OutputKind::Output,
                        entries
                            .iter()
                            .map(|e| e.name.as_str())
                            .collect::<Vec<_>>()
                            .join("  "),
                    ));
                }
            }
            Err(e) => reply
                .lines
                .push(ShellLine::new(OutputKind::Error, format!("ls: {e}"))),
        }
    }

    async fn cmd_cd(
        &mut self,
        args: &[&str],
        backend: &dyn DeviceBackend,
        token: &str,
        reply: &mut ShellReply,
    ) {
        let target = args.first().copied().unwrap_or("/");
        let resolved = match self.abs(target) {
            Ok(p) => p,
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("cd: {e}")));
                return;
            }
        };
        match backend.list(&resolved, token).await {
            Ok(_) => self.cwd = resolved,
            Err(_) => reply.lines.push(ShellLine::new(
                OutputKind::Error,
                format!("cd: no such file or directory: {target}"),
            )),
        }
    }

    async fn cmd_cat(
        &self,
        args: &[&str],
        backend: &dyn DeviceBackend,
        token: &str,
        reply: &mut ShellReply,
    ) {
        let Some(target) = args.first() else {
            reply
                .lines
                .push(ShellLine::new(OutputKind::Error, "usage: cat <file>"));
            return;
        };
        let path = match self.abs(target) {
            Ok(p) => p,
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("cat: {e}")));
                return;
            }
        };
        let print = |reply: &mut ShellReply, text: &str| {
            for line in text.lines() {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Output, line.to_string()));
            }
        };
        match backend.read_text(&path, token).await {
            Ok(text) => print(reply, &text),
            Err(e) if matches!(e, crate::domain::DeviceError::NotFound(_)) => {
                // Prototype convenience: on a miss, `cat note.md` also looks
                // for one uniquely named file anywhere below the cwd.
                let file_name = target.rsplit('/').next().unwrap_or(target);
                let hit = backend.walk(&self.cwd, token).await.ok().and_then(|all| {
                    let mut hits = all.iter().filter(|e| e.name == file_name && !e.dir);
                    let first = hits.next().cloned();
                    hits.next().is_none().then_some(first).flatten()
                });
                match hit {
                    Some(entry) => match backend.read_text(&entry.path, token).await {
                        Ok(text) => print(reply, &text),
                        Err(inner) => reply
                            .lines
                            .push(ShellLine::new(OutputKind::Error, format!("cat: {inner}"))),
                    },
                    None => reply
                        .lines
                        .push(ShellLine::new(OutputKind::Error, format!("cat: {e}"))),
                }
            }
            Err(e) => reply
                .lines
                .push(ShellLine::new(OutputKind::Error, format!("cat: {e}"))),
        }
    }

    async fn cmd_mkdir(
        &self,
        args: &[&str],
        backend: &dyn DeviceBackend,
        token: &str,
        reply: &mut ShellReply,
    ) -> bool {
        let Some(target) = args.first() else {
            reply
                .lines
                .push(ShellLine::new(OutputKind::Error, "usage: mkdir <name>"));
            return false;
        };
        let path = match self.abs(target) {
            Ok(p) => p,
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("mkdir: {e}")));
                return false;
            }
        };
        match backend.mkdir(&path, token).await {
            Ok(()) => {
                reply.lines.push(ShellLine::new(
                    OutputKind::Success,
                    format!(
                        "Created directory: {}",
                        path.rsplit('/').next().unwrap_or(&path)
                    ),
                ));
                true
            }
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("mkdir: {e}")));
                false
            }
        }
    }

    async fn cmd_touch(
        &self,
        args: &[&str],
        backend: &dyn DeviceBackend,
        token: &str,
        reply: &mut ShellReply,
    ) -> bool {
        let Some(target) = args.first() else {
            reply
                .lines
                .push(ShellLine::new(OutputKind::Error, "usage: touch <name>"));
            return false;
        };
        let path = match self.abs(target) {
            Ok(p) => p,
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("touch: {e}")));
                return false;
            }
        };
        match backend.touch(&path, token).await {
            Ok(()) => {
                reply.lines.push(ShellLine::new(
                    OutputKind::Success,
                    format!(
                        "Created empty file: {}",
                        path.rsplit('/').next().unwrap_or(&path)
                    ),
                ));
                true
            }
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("touch: {e}")));
                false
            }
        }
    }

    async fn cmd_rm(
        &self,
        args: &[&str],
        backend: &dyn DeviceBackend,
        token: &str,
        reply: &mut ShellReply,
    ) -> bool {
        let Some(target) = args.first() else {
            reply
                .lines
                .push(ShellLine::new(OutputKind::Error, "usage: rm <path>"));
            return false;
        };
        let path = match self.abs(target) {
            Ok(p) => p,
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("rm: {e}")));
                return false;
            }
        };
        match backend.remove(&path, token).await {
            Ok(()) => {
                reply.lines.push(ShellLine::new(
                    OutputKind::Success,
                    format!("Removed '{target}'"),
                ));
                true
            }
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("rm: {e}")));
                false
            }
        }
    }

    /// `cp <src> <dest-folder>` — copies into the destination folder; name
    /// conflicts are deduplicated by the device.
    async fn cmd_cp(
        &self,
        args: &[&str],
        backend: &dyn DeviceBackend,
        token: &str,
        reply: &mut ShellReply,
    ) -> bool {
        let Some((src, dest)) = two_args(args) else {
            reply.lines.push(ShellLine::new(
                OutputKind::Error,
                "usage: cp <src> <dest-folder>",
            ));
            return false;
        };
        let from = match self.abs(src) {
            Ok(p) => p,
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("cp: {e}")));
                return false;
            }
        };
        let folder = match self.abs(dest) {
            Ok(p) => p,
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("cp: {e}")));
                return false;
            }
        };
        match backend.cp(&from, &folder, token).await {
            Ok(()) => {
                reply.lines.push(ShellLine::new(
                    OutputKind::Success,
                    format!("Copied '{src}' -> '{}'", display_path(&folder)),
                ));
                true
            }
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("cp: {e}")));
                false
            }
        }
    }

    /// `mv <src> <dest>` — a destination that lists OK is a folder (move
    /// inside it under the source's own name); anything else is treated as
    /// the new full path (rename).
    async fn cmd_mv(
        &self,
        args: &[&str],
        backend: &dyn DeviceBackend,
        token: &str,
        reply: &mut ShellReply,
    ) -> bool {
        let Some((src, dest)) = two_args(args) else {
            reply
                .lines
                .push(ShellLine::new(OutputKind::Error, "usage: mv <src> <dest>"));
            return false;
        };
        let from = match self.abs(src) {
            Ok(p) => p,
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("mv: {e}")));
                return false;
            }
        };
        let target = match self.abs(dest) {
            Ok(p) => p,
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("mv: {e}")));
                return false;
            }
        };
        let name = from.rsplit('/').next().unwrap_or(&from);
        let to = if backend.list(&target, token).await.is_ok() {
            format!("{}/{name}", target.trim_end_matches('/'))
        } else {
            target
        };
        match backend.mv(&from, &to, token).await {
            Ok(()) => {
                reply.lines.push(ShellLine::new(
                    OutputKind::Success,
                    format!("Moved '{src}' -> '{}'", display_path(&to)),
                ));
                true
            }
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("mv: {e}")));
                false
            }
        }
    }

    async fn cmd_tree(&self, backend: &dyn DeviceBackend, token: &str, reply: &mut ShellReply) {
        let entries = match backend.list(&self.cwd, token).await {
            Ok(e) => e,
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("tree: {e}")));
                return;
            }
        };
        reply
            .lines
            .push(ShellLine::new(OutputKind::Output, display_path(&self.cwd)));
        for (i, e) in entries.iter().enumerate() {
            let connector = if i + 1 == entries.len() {
                "└── "
            } else {
                "├── "
            };
            let suffix = match (e.dir, e.item_count) {
                (true, Some(n)) => format!(" ({n} items)"),
                _ => String::new(),
            };
            reply.lines.push(ShellLine::new(
                OutputKind::Output,
                format!("{connector}{}{suffix}", e.name),
            ));
        }
    }

    /// `stat <path>` — size/type/modified read off the parent listing (the
    /// `Entry` model already carries the stat fields).
    async fn cmd_stat(
        &self,
        args: &[&str],
        backend: &dyn DeviceBackend,
        token: &str,
        reply: &mut ShellReply,
    ) {
        let Some(target) = args.first() else {
            reply
                .lines
                .push(ShellLine::new(OutputKind::Error, "usage: stat <path>"));
            return;
        };
        let path = match self.abs(target) {
            Ok(p) => p,
            Err(e) => {
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Error, format!("stat: {e}")));
                return;
            }
        };
        let Some((parent, _)) = path.rsplit_once('/') else {
            reply
                .lines
                .push(ShellLine::new(OutputKind::Error, format!("stat: {path}")));
            return;
        };
        let entry = backend
            .list(parent, token)
            .await
            .ok()
            .and_then(|entries| entries.into_iter().find(|e| e.path == path));
        let Some(entry) = entry else {
            reply.lines.push(ShellLine::new(
                OutputKind::Error,
                format!("stat: no such file or directory: {target}"),
            ));
            return;
        };
        let kind = if entry.dir {
            "directory".to_string()
        } else {
            entry
                .mime_type
                .clone()
                .unwrap_or_else(|| FileCategory::of(&entry).to_string())
        };
        reply.lines.push(ShellLine::new(
            OutputKind::Output,
            format!("  File: {}", entry.name),
        ));
        reply.lines.push(ShellLine::new(
            OutputKind::Output,
            format!(
                "  Size: {} ({} bytes)",
                format_bytes(entry.size, 1),
                entry.size
            ),
        ));
        reply.lines.push(ShellLine::new(
            OutputKind::Output,
            format!("  Type: {kind}"),
        ));
        if let Some(count) = entry.item_count {
            reply.lines.push(ShellLine::new(
                OutputKind::Output,
                format!("  Items: {count}"),
            ));
        }
        reply.lines.push(ShellLine::new(
            OutputKind::Output,
            format!("  Modified: {}", format_date(entry.mtime)),
        ));
    }

    /// `search <query>` — plain filename/path search over the whole tree
    /// (the AI query's lexical sibling).
    async fn cmd_search(
        &self,
        args: &[&str],
        backend: &dyn DeviceBackend,
        token: &str,
        search: &dyn SearchProvider,
        reply: &mut ShellReply,
    ) {
        let query = args.join(" ");
        if query.trim().is_empty() {
            reply
                .lines
                .push(ShellLine::new(OutputKind::Error, "usage: search <query>"));
            return;
        }
        let entries = backend.walk(STORAGE_ROOT, token).await.unwrap_or_default();
        match search.search(&query, &entries, &self.cwd).await {
            Ok(res) => {
                reply.lines.push(ShellLine::new(
                    OutputKind::Output,
                    format!("Search: \"{query}\" — {} match(es)", res.matches.len()),
                ));
                for m in res.matches.iter().take(20) {
                    reply.lines.push(ShellLine::new(
                        OutputKind::Output,
                        format!("  {} — {}", m.path, m.reason),
                    ));
                }
            }
            Err(e) => reply
                .lines
                .push(ShellLine::new(OutputKind::Error, format!("search: {e}"))),
        }
    }

    async fn cmd_ai(
        &self,
        args: &[&str],
        backend: &dyn DeviceBackend,
        token: &str,
        search: &dyn SearchProvider,
        reply: &mut ShellReply,
    ) {
        let query = args.join(" ");
        if query.trim().is_empty() {
            reply.lines.push(ShellLine::new(
                OutputKind::Error,
                "usage: ai <query>  (e.g. ai camera photos)",
            ));
            return;
        }
        let entries = backend.walk(STORAGE_ROOT, token).await.unwrap_or_default();
        match search.search(&query, &entries, &self.cwd).await {
            Ok(res) => {
                reply.lines.push(ShellLine::new(
                    OutputKind::Ai,
                    format!("AI Search: \"{query}\""),
                ));
                for m in &res.matches {
                    reply.lines.push(ShellLine::new(
                        OutputKind::Ai,
                        format!("• {} — {} ({})", m.path, m.reason, m.confidence.as_str()),
                    ));
                }
                if let Some(warning) = &res.warning {
                    reply
                        .lines
                        .push(ShellLine::new(OutputKind::Error, warning.clone()));
                }
                reply
                    .lines
                    .push(ShellLine::new(OutputKind::Ai, res.summary));
            }
            Err(e) => reply
                .lines
                .push(ShellLine::new(OutputKind::Error, format!("ai: {e}"))),
        }
    }
}

/// Exactly-two-argument extractor for `cp`/`mv`.
fn two_args<'a>(args: &'a [&'a str]) -> Option<(&'a str, &'a str)> {
    match args {
        [a, b] => Some((a, b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockDevice;
    use crate::search::HeuristicSearchProvider;
    use futures::executor::block_on;

    const TOKEN: &str = "7f9c2d1b84e035a6bc8910fedcba4321";
    const NOW: i64 = 1_772_000_000_000;

    fn fixture() -> (MockDevice, ShellSession) {
        let device = MockDevice::new(NOW);
        let session = ShellSession::new("Pixel 9 Pro (Tensor G4)");
        (device, session)
    }

    fn text(reply: &ShellReply) -> String {
        reply
            .lines
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn prompt_uses_model_host_and_display_path() {
        let (_, s) = fixture();
        assert!(s.prompt().starts_with("android18@pixel-9-pro"));
        assert!(s.prompt().contains(":~$"));
    }

    #[test]
    fn ls_lists_root_names() {
        let (d, mut s) = fixture();
        let reply = block_on(s.execute("ls", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("DCIM"));
        assert!(text(&reply).contains("Documents"));
    }

    #[test]
    fn cd_updates_cwd_and_ls_l_is_long() {
        let (d, mut s) = fixture();
        block_on(s.execute("cd DCIM", &d, TOKEN, &HeuristicSearchProvider));
        assert_eq!(s.cwd(), format!("{STORAGE_ROOT}/DCIM"));
        let reply = block_on(s.execute("ls -l", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("total 2"));
        assert!(text(&reply).contains("drwxr-xr-x"));
    }

    #[test]
    fn cd_missing_dir_reports_error() {
        let (d, mut s) = fixture();
        let reply = block_on(s.execute("cd nope", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("cd: no such file or directory: nope"));
        assert_eq!(s.cwd(), STORAGE_ROOT);
    }

    #[test]
    fn cat_prints_content_and_rejects_directories() {
        let (d, mut s) = fixture();
        let reply = block_on(s.execute(
            "cat Product_Roadmap_Q4.md",
            &d,
            TOKEN,
            &HeuristicSearchProvider,
        ));
        assert!(text(&reply).contains("# Android18 Q4 Roadmap"));
        let reply = block_on(s.execute("cat DCIM", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("is a directory"));
    }

    #[test]
    fn mkdir_touch_rm_round_trip() {
        let (d, mut s) = fixture();
        block_on(s.execute("mkdir Work", &d, TOKEN, &HeuristicSearchProvider));
        block_on(s.execute("cd Work", &d, TOKEN, &HeuristicSearchProvider));
        block_on(s.execute("touch notes.md", &d, TOKEN, &HeuristicSearchProvider));
        let reply = block_on(s.execute("ls", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("notes.md"));
        block_on(s.execute("rm notes.md", &d, TOKEN, &HeuristicSearchProvider));
        let reply = block_on(s.execute("ls", &d, TOKEN, &HeuristicSearchProvider));
        assert!(!text(&reply).contains("notes.md"));
    }

    #[test]
    fn tree_shows_connectors() {
        let (d, mut s) = fixture();
        let reply = block_on(s.execute("tree", &d, TOKEN, &HeuristicSearchProvider));
        let t = text(&reply);
        assert!(t.contains("├── ") && t.contains("└── "));
        assert!(t.contains("DCIM (2 items)"));
    }

    #[test]
    fn ai_search_finds_photos() {
        let (d, mut s) = fixture();
        let reply = block_on(s.execute("ai camera photos", &d, TOKEN, &HeuristicSearchProvider));
        let t = text(&reply);
        assert!(t.contains("AI Search: \"camera photos\""));
        assert!(t.contains("IMG_20261001_143022.jpg"));
    }

    #[test]
    fn unknown_command_and_history() {
        let (d, mut s) = fixture();
        let reply = block_on(s.execute("frobnicate", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("command not found: frobnicate"));
        assert_eq!(s.history(), ["frobnicate"]);
    }

    #[test]
    fn clear_flag_set() {
        let (d, mut s) = fixture();
        let reply = block_on(s.execute("clear", &d, TOKEN, &HeuristicSearchProvider));
        assert!(reply.clear);
    }

    #[test]
    fn cp_copies_into_folder_and_flags_mutated() {
        let (d, mut s) = fixture();
        block_on(s.execute("touch notes.md", &d, TOKEN, &HeuristicSearchProvider));
        let reply =
            block_on(s.execute("cp notes.md Documents", &d, TOKEN, &HeuristicSearchProvider));
        assert!(reply.mutated);
        assert!(text(&reply).contains("Copied"));
        let reply = block_on(s.execute("ls Documents", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("notes.md"));
        // The original stays behind after a copy.
        let reply = block_on(s.execute("ls", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("notes.md"));
    }

    #[test]
    fn cp_usage_error_is_not_a_mutation() {
        let (d, mut s) = fixture();
        let reply = block_on(s.execute("cp only-one-arg", &d, TOKEN, &HeuristicSearchProvider));
        assert!(!reply.mutated);
        assert!(text(&reply).contains("usage: cp"));
    }

    #[test]
    fn mv_renames_when_dest_is_not_a_folder() {
        let (d, mut s) = fixture();
        block_on(s.execute("touch base.md", &d, TOKEN, &HeuristicSearchProvider));
        let reply = block_on(s.execute("mv base.md Plan.md", &d, TOKEN, &HeuristicSearchProvider));
        assert!(reply.mutated);
        assert!(text(&reply).contains("Moved"));
        let reply = block_on(s.execute("ls", &d, TOKEN, &HeuristicSearchProvider));
        let listing = text(&reply);
        assert!(listing.contains("Plan.md"));
        assert!(!listing.contains("base.md"));
    }

    #[test]
    fn mv_into_folder_keeps_name() {
        let (d, mut s) = fixture();
        block_on(s.execute("touch keep.md", &d, TOKEN, &HeuristicSearchProvider));
        block_on(s.execute("mv keep.md Documents", &d, TOKEN, &HeuristicSearchProvider));
        let reply = block_on(s.execute("ls Documents", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("keep.md"));
    }

    #[test]
    fn dir_and_ll_alias_ls_and_cls_clears() {
        let (d, mut s) = fixture();
        let reply = block_on(s.execute("dir", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("DCIM"));
        let reply = block_on(s.execute("ll", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("total"));
        let reply = block_on(s.execute("cls", &d, TOKEN, &HeuristicSearchProvider));
        assert!(reply.clear);
    }

    #[test]
    fn search_lists_matches_without_mutating() {
        let (d, mut s) = fixture();
        let reply = block_on(s.execute("search roadmap", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("Search: \"roadmap\""));
        assert!(!reply.mutated);
    }

    #[test]
    fn stat_prints_size_type_modified() {
        let (d, mut s) = fixture();
        block_on(s.execute("touch meta.md", &d, TOKEN, &HeuristicSearchProvider));
        let reply = block_on(s.execute("stat meta.md", &d, TOKEN, &HeuristicSearchProvider));
        let output = text(&reply);
        assert!(output.contains("File: meta.md"));
        assert!(output.contains("Size:"));
        assert!(output.contains("Type:"));
        assert!(output.contains("Modified:"));
        assert!(!reply.mutated);
        // Directories report their item count too.
        let reply = block_on(s.execute("stat DCIM", &d, TOKEN, &HeuristicSearchProvider));
        assert!(text(&reply).contains("Items:"));
    }

    #[test]
    fn mkdir_flags_mutated_and_cat_does_not() {
        let (d, mut s) = fixture();
        let reply = block_on(s.execute("mkdir Work", &d, TOKEN, &HeuristicSearchProvider));
        assert!(reply.mutated);
        let reply = block_on(s.execute(
            "cat Product_Roadmap_Q4.md",
            &d,
            TOKEN,
            &HeuristicSearchProvider,
        ));
        assert!(!reply.mutated);
    }
}
