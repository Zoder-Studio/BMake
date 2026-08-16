use anyhow::Result;
use std::path::PathBuf;

const NVIM_SYNTAX_BM: &str = include_str!("../../../editors/nvim/syntax/bmake.vim");
const NVIM_SYNTAX_KTS: &str = include_str!("../../../editors/nvim/syntax/bmake_kts.vim");
const NVIM_FTDETECT: &str = include_str!("../../../editors/nvim/ftdetect/bmake.vim");
const NANO_SYNTAX: &str = include_str!("../../../editors/nano/bmake.nanorc");

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Detects $EDITOR and, for editors with an official BMake integration,
/// installs it into the user's standard config location. For editors that
/// can't be set up with a plain file copy (VS Code, Helix) it only prints
/// instructions. Unknown editors get an informational message and nothing
/// is touched — never an error, never a silent no-op.
pub fn detect_and_setup() {
    let Ok(editor) = std::env::var("EDITOR") else {
        return;
    };
    let editor_name = editor.rsplit('/').next().unwrap_or(&editor);

    let result = match editor_name {
        "nvim" | "vim" => setup_nvim(),
        "nano" => setup_nano(),
        "code" | "code-oss" | "codium" => {
            println!(" Detected $EDITOR={} — VS Code syntax highlighting for BMake is available.", editor);
            println!(" Install it from editors/vscode/bmake in the BMake repo (package with 'vsce package' then 'code --install-extension', or copy the folder into your VS Code extensions directory).");
            Ok(())
        }
        "hx" | "helix" => {
            println!(" Detected $EDITOR={} — Helix syntax highlighting for BMake requires building a tree-sitter grammar.", editor);
            println!(" See editors/helix/README.md in the BMake repo for the grammar source and languages.toml entry to add.");
            Ok(())
        }
        other => {
            println!(" $EDITOR is set to '{}' — BMake doesn't have an official syntax highlighting integration for this editor yet. Nothing was changed.", other);
            Ok(())
        }
    };

    if let Err(e) = result {
        println!(" Editor integration setup skipped: {}", e);
    }
}

fn setup_nvim() -> Result<()> {
    let Some(home) = home_dir() else { return Ok(()) };
    let config = home.join(".config/nvim");
    let syntax_dir = config.join("syntax");
    let ftdetect_dir = config.join("ftdetect");
    std::fs::create_dir_all(&syntax_dir)?;
    std::fs::create_dir_all(&ftdetect_dir)?;

    write_if_different(&syntax_dir.join("bmake.vim"), NVIM_SYNTAX_BM)?;
    write_if_different(&syntax_dir.join("bmake_kts.vim"), NVIM_SYNTAX_KTS)?;
    write_if_different(&ftdetect_dir.join("bmake.vim"), NVIM_FTDETECT)?;

    println!(" Neovim BMake syntax highlighting installed to {}", config.display());
    Ok(())
}

fn setup_nano() -> Result<()> {
    let Some(home) = home_dir() else { return Ok(()) };
    let nano_dir = home.join(".nano");
    std::fs::create_dir_all(&nano_dir)?;
    let target = nano_dir.join("bmake.nanorc");
    write_if_different(&target, NANO_SYNTAX)?;

    let nanorc = home.join(".nanorc");
    let already_included = std::fs::read_to_string(&nanorc)
        .map(|c| c.contains(&*target.display().to_string()))
        .unwrap_or(false);

    if !already_included {
        let mut content = std::fs::read_to_string(&nanorc).unwrap_or_default();
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&format!("include \"{}\"\n", target.display()));
        std::fs::write(&nanorc, content)?;
    }

    println!(" nano BMake syntax highlighting installed and included from {}", nanorc.display());
    Ok(())
}

fn write_if_different(path: &std::path::Path, content: &str) -> Result<()> {
    if std::fs::read_to_string(path).map(|c| c == content).unwrap_or(false) {
        return Ok(());
    }
    std::fs::write(path, content)?;
    Ok(())
}