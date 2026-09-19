//! `2bec completion` and `2bec man`: shell completions and manual pages
//! generated from the command definitions, so they never drift from `--help`.

use anyhow::{Context as _, Result};
use clap::Args;
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Args, Debug)]
#[command(after_help = "\
Examples:
  2bec completion bash > ~/.local/share/bash-completion/completions/2bec
  2bec completion zsh > \"${fpath[1]}/_2bec\"
  2bec completion fish > ~/.config/fish/completions/2bec.fish
  2bec completion powershell >> $PROFILE

Restart the shell (or source the file) for the completions to take effect.")]
pub struct CompletionArgs {
    /// Shell to generate the completion script for
    #[arg(value_enum)]
    shell: Shell,
}

#[derive(Args, Debug)]
#[command(after_help = "\
Examples:
  2bec man | man -l -                  # read the main page now
  2bec man --dir ~/.local/share/man/man1
  sudo 2bec man --dir /usr/local/share/man/man1

With --dir, one page is written per command (2bec.1, 2bec-runs.1,
2bec-runs-list.1, ...); run `mandb` afterwards if `man 2bec` does not find them.")]
pub struct ManArgs {
    /// Write a page per command into this directory instead of printing the main page
    #[arg(long, value_name = "DIR")]
    dir: Option<PathBuf>,
}

/// Print the completion script for `args.shell` on stdout.
pub fn completion(mut cmd: clap::Command, args: CompletionArgs) -> Result<()> {
    let name = cmd.get_name().to_string();
    let mut buf = Vec::new();
    clap_complete::generate(args.shell, &mut cmd, name, &mut buf);
    print_all(&buf)
}

/// Write to stdout; a reader that closed early (`| head`) is not an error.
fn print_all(bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    match out.write_all(bytes).and_then(|()| out.flush()) {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other.context("cannot write to stdout"),
    }
}

/// Print the main manual page, or write the whole set into `--dir`.
pub fn man(cmd: clap::Command, args: ManArgs) -> Result<()> {
    let cmd = cmd
        .disable_help_subcommand(true)
        .version(env!("CARGO_PKG_VERSION"));
    match args.dir {
        Some(dir) => {
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
            clap_mangen::generate_to(cmd, &dir)
                .with_context(|| format!("cannot write the manual pages to {}", dir.display()))?;
            eprintln!("Wrote the manual pages to {}", dir.display());
        }
        None => {
            let mut buf = Vec::new();
            clap_mangen::Man::new(cmd)
                .render(&mut buf)
                .context("cannot render the manual page")?;
            print_all(&buf)?;
        }
    }
    Ok(())
}
