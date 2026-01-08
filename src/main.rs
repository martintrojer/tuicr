mod app;
mod error;
mod git;
mod input;
mod model;
mod output;
mod persistence;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::App;
use input::{Action, map_key_to_action};

fn main() -> anyhow::Result<()> {
    // Setup panic hook to restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    // Initialize app
    let mut app = match App::new() {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!("\nMake sure you're in a git repository with uncommitted changes.");
            std::process::exit(1);
        }
    };

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Main loop
    loop {
        // Render
        terminal.draw(|frame| {
            ui::render(frame, &app);
        })?;

        // Handle events
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let action = map_key_to_action(key, app.input_mode);

                match action {
                    Action::Quit => {
                        app.should_quit = true;
                    }
                    Action::ScrollDown(n) => app.scroll_down(n),
                    Action::ScrollUp(n) => app.scroll_up(n),
                    Action::HalfPageDown => app.scroll_down(15),
                    Action::HalfPageUp => app.scroll_up(15),
                    Action::PageDown => app.scroll_down(30),
                    Action::PageUp => app.scroll_up(30),
                    Action::GoToTop => app.jump_to_file(0),
                    Action::GoToBottom => {
                        let last = app.file_count().saturating_sub(1);
                        app.jump_to_file(last);
                    }
                    Action::NextFile => app.next_file(),
                    Action::PrevFile => app.prev_file(),
                    Action::ToggleReviewed => app.toggle_reviewed(),
                    Action::ToggleFocus => {
                        app.focused_panel = match app.focused_panel {
                            app::FocusedPanel::FileList => app::FocusedPanel::Diff,
                            app::FocusedPanel::Diff => app::FocusedPanel::FileList,
                        };
                    }
                    Action::FocusFileList => {
                        app.focused_panel = app::FocusedPanel::FileList;
                    }
                    Action::FocusDiff => {
                        app.focused_panel = app::FocusedPanel::Diff;
                    }
                    Action::SelectFile => {
                        if app.focused_panel == app::FocusedPanel::FileList {
                            app.jump_to_file(app.file_list_state.selected);
                        }
                    }
                    Action::ToggleHelp => app.toggle_help(),
                    Action::EnterCommandMode => app.enter_command_mode(),
                    Action::ExitMode => {
                        if app.input_mode == app::InputMode::Command {
                            app.exit_command_mode();
                        }
                    }
                    Action::InsertChar(c) => {
                        if app.input_mode == app::InputMode::Command {
                            app.command_buffer.push(c);
                        }
                    }
                    Action::DeleteChar => {
                        if app.input_mode == app::InputMode::Command {
                            app.command_buffer.pop();
                        }
                    }
                    Action::SubmitInput => {
                        if app.input_mode == app::InputMode::Command {
                            let cmd = app.command_buffer.trim().to_string();
                            match cmd.as_str() {
                                "q" | "quit" => app.should_quit = true,
                                "w" | "write" => {
                                    // TODO: implement save
                                    app.set_message("Save not yet implemented");
                                }
                                "wq" => {
                                    // TODO: implement save
                                    app.should_quit = true;
                                }
                                "e" | "export" => {
                                    // TODO: implement export
                                    app.set_message("Export not yet implemented");
                                }
                                _ => {
                                    app.set_message(format!("Unknown command: {}", cmd));
                                }
                            }
                            app.exit_command_mode();
                        }
                    }
                    _ => {}
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(())
}
