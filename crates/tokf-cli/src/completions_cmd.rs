use std::io;

use clap::CommandFactory;
use clap_complete::Generator;

use crate::Cli;

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum ShellChoice {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell")]
    PowerShell,
    Elvish,
    Nushell,
}

pub fn cmd_completions(shell: ShellChoice) -> i32 {
    let mut cmd = Cli::command();
    let bin_name = "tokf";
    generate_to_writer(shell, &mut cmd, bin_name, &mut io::stdout());
    print_install_hint(shell);
    0
}

fn generate_to_writer(
    shell: ShellChoice,
    cmd: &mut clap::Command,
    name: &str,
    out: &mut dyn io::Write,
) {
    match shell {
        ShellChoice::Bash => clap_complete::generate(clap_complete::Shell::Bash, cmd, name, out),
        ShellChoice::Zsh => clap_complete::generate(clap_complete::Shell::Zsh, cmd, name, out),
        ShellChoice::Fish => clap_complete::generate(clap_complete::Shell::Fish, cmd, name, out),
        ShellChoice::PowerShell => {
            clap_complete::generate(clap_complete::Shell::PowerShell, cmd, name, out);
        }
        ShellChoice::Elvish => {
            clap_complete::generate(clap_complete::Shell::Elvish, cmd, name, out);
        }
        ShellChoice::Nushell => {
            cmd.set_bin_name(name);
            cmd.build();
            clap_complete_nushell::Nushell.generate(cmd, out);
        }
    }
}

fn print_install_hint(shell: ShellChoice) {
    let hint = match shell {
        ShellChoice::Bash => "# Add to ~/.bashrc:\n#   eval \"$(tokf completions bash)\"",
        ShellChoice::Zsh => "# Add to ~/.zshrc:\n#   eval \"$(tokf completions zsh)\"",
        ShellChoice::Fish => {
            "# Save to fish completions dir:\n#   tokf completions fish > ~/.config/fish/completions/tokf.fish"
        }
        ShellChoice::PowerShell => {
            "# Add to your PowerShell profile:\n#   tokf completions powershell | Out-String | Invoke-Expression"
        }
        ShellChoice::Elvish => {
            "# Add to ~/.elvish/rc.elv:\n#   eval (tokf completions elvish | slurp)"
        }
        ShellChoice::Nushell => {
            "# Save and source in your config:\n#   tokf completions nushell | save -f ~/.config/nushell/tokf.nu\n#   source ~/.config/nushell/tokf.nu"
        }
    };
    eprintln!("\n{hint}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_completions(shell: ShellChoice) -> Vec<u8> {
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        generate_to_writer(shell, &mut cmd, "tokf", &mut buf);
        buf
    }

    #[test]
    fn all_shells_produce_output() {
        let shells = [
            ShellChoice::Bash,
            ShellChoice::Zsh,
            ShellChoice::Fish,
            ShellChoice::PowerShell,
            ShellChoice::Elvish,
            ShellChoice::Nushell,
        ];
        for shell in shells {
            let buf = generate_completions(shell);
            assert!(!buf.is_empty(), "expected non-empty output for {shell:?}");
            let output = String::from_utf8_lossy(&buf);
            assert!(
                output.contains("tokf"),
                "expected 'tokf' in completion output for {shell:?}, got:\n{output}"
            );
        }
    }
}
