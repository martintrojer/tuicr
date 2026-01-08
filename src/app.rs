use std::path::PathBuf;

use crate::error::Result;
use crate::git::{RepoInfo, get_working_tree_diff};
use crate::model::{DiffFile, FileStatus, ReviewSession};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Comment,
    Command,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPanel {
    FileList,
    Diff,
}

pub struct App {
    pub repo_info: RepoInfo,
    pub session: ReviewSession,
    pub diff_files: Vec<DiffFile>,

    pub input_mode: InputMode,
    pub focused_panel: FocusedPanel,

    pub file_list_state: FileListState,
    pub diff_state: DiffState,
    pub command_buffer: String,
    pub comment_buffer: String,

    pub should_quit: bool,
    pub dirty: bool,
    pub message: Option<String>,
}

#[derive(Debug, Default)]
pub struct FileListState {
    pub selected: usize,
    pub offset: usize,
}

#[derive(Debug, Default)]
pub struct DiffState {
    pub scroll_offset: usize,
    pub cursor_line: usize,
    pub current_file_idx: usize,
}

impl App {
    pub fn new() -> Result<Self> {
        let repo_info = RepoInfo::discover()?;
        let diff_files = get_working_tree_diff(&repo_info.repo)?;

        let mut session =
            ReviewSession::new(repo_info.root_path.clone(), repo_info.head_commit.clone());

        for file in &diff_files {
            let path = file.display_path().clone();
            session.add_file(path, file.status);
        }

        Ok(Self {
            repo_info,
            session,
            diff_files,
            input_mode: InputMode::Normal,
            focused_panel: FocusedPanel::Diff,
            file_list_state: FileListState::default(),
            diff_state: DiffState::default(),
            command_buffer: String::new(),
            comment_buffer: String::new(),
            should_quit: false,
            dirty: false,
            message: None,
        })
    }

    pub fn current_file(&self) -> Option<&DiffFile> {
        self.diff_files.get(self.diff_state.current_file_idx)
    }

    pub fn current_file_path(&self) -> Option<&PathBuf> {
        self.current_file().map(|f| f.display_path())
    }

    pub fn toggle_reviewed(&mut self) {
        if let Some(path) = self.current_file_path().cloned() {
            if let Some(review) = self.session.get_file_mut(&path) {
                review.reviewed = !review.reviewed;
                self.dirty = true;
            }
        }
    }

    pub fn is_current_file_reviewed(&self) -> bool {
        self.current_file_path()
            .and_then(|p| self.session.files.get(p))
            .is_some_and(|r| r.reviewed)
    }

    pub fn file_count(&self) -> usize {
        self.diff_files.len()
    }

    pub fn reviewed_count(&self) -> usize {
        self.session.reviewed_count()
    }

    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.message = Some(msg.into());
    }

    pub fn clear_message(&mut self) {
        self.message = None;
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.diff_state.scroll_offset = self.diff_state.scroll_offset.saturating_add(lines);
        self.update_current_file_from_scroll();
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.diff_state.scroll_offset = self.diff_state.scroll_offset.saturating_sub(lines);
        self.update_current_file_from_scroll();
    }

    pub fn jump_to_file(&mut self, idx: usize) {
        if idx < self.diff_files.len() {
            self.diff_state.current_file_idx = idx;
            self.diff_state.scroll_offset = self.calculate_file_scroll_offset(idx);
            self.file_list_state.selected = idx;
        }
    }

    pub fn next_file(&mut self) {
        let next =
            (self.diff_state.current_file_idx + 1).min(self.diff_files.len().saturating_sub(1));
        self.jump_to_file(next);
    }

    pub fn prev_file(&mut self) {
        let prev = self.diff_state.current_file_idx.saturating_sub(1);
        self.jump_to_file(prev);
    }

    fn calculate_file_scroll_offset(&self, file_idx: usize) -> usize {
        let mut offset = 0;
        for (i, file) in self.diff_files.iter().enumerate() {
            if i == file_idx {
                break;
            }
            offset += self.file_render_height(file);
        }
        offset
    }

    fn file_render_height(&self, file: &DiffFile) -> usize {
        let header_lines = 2;
        let content_lines: usize = file.hunks.iter().map(|h| h.lines.len() + 1).sum();
        header_lines + content_lines.max(1)
    }

    fn update_current_file_from_scroll(&mut self) {
        let mut cumulative = 0;
        for (i, file) in self.diff_files.iter().enumerate() {
            let height = self.file_render_height(file);
            if cumulative + height > self.diff_state.scroll_offset {
                self.diff_state.current_file_idx = i;
                self.file_list_state.selected = i;
                return;
            }
            cumulative += height;
        }
        if !self.diff_files.is_empty() {
            self.diff_state.current_file_idx = self.diff_files.len() - 1;
            self.file_list_state.selected = self.diff_files.len() - 1;
        }
    }

    pub fn enter_command_mode(&mut self) {
        self.input_mode = InputMode::Command;
        self.command_buffer.clear();
    }

    pub fn exit_command_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.command_buffer.clear();
    }

    pub fn enter_comment_mode(&mut self) {
        self.input_mode = InputMode::Comment;
        self.comment_buffer.clear();
    }

    pub fn exit_comment_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.comment_buffer.clear();
    }

    pub fn toggle_help(&mut self) {
        if self.input_mode == InputMode::Help {
            self.input_mode = InputMode::Normal;
        } else {
            self.input_mode = InputMode::Help;
        }
    }
}
