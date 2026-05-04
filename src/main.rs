use anyhow::{Context, Result};
use chrono::Local;
use console::{style, Term};
use dialoguer::{theme::ColorfulTheme, FuzzySelect, Input, MultiSelect, Confirm};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use which::which;

struct Lang {
    welcome: &'static str,
    repo_prompt: &'static str,
    cloning: &'static str,
    mode_prompt: &'static str,
    modes: [&'static str; 2],
    path_prompt: &'static str,
    select_prompt: &'static str,
    copying: &'static str,
    backup_msg: &'static str,
    cleaning: &'static str,
    done: &'static str,
    error_git: &'static str,
    dep_prompt: &'static str,
}

const RU: Lang = Lang {
    welcome: "DOTMASTER // Системный установщик",
    repo_prompt: "URL репозитория",
    cloning: "Загрузка временных объектов...",
    mode_prompt: "Место установки",
    modes: ["Системный каталог (~/.config)", "Пользовательский путь"],
    path_prompt: "Укажите путь",
    select_prompt: "Выберите компоненты (Space - выбор, Enter - подтверждение)",
    copying: "Установка",
    backup_msg: "Резервная копия:",
    cleaning: "Очистка кэша...",
    done: "Установка завершена успешно.",
    error_git: "Ошибка клонирования репозитория.",
    dep_prompt: "Пакет '{}' не найден. Установить его через yay?",
};

const EN: Lang = Lang {
    welcome: "DOTMASTER // System Installer",
    repo_prompt: "Repository URL",
    cloning: "Downloading temporary objects...",
    mode_prompt: "Installation target",
    modes: ["System config (~/.config)", "Custom path"],
    path_prompt: "Specify path",
    select_prompt: "Select components (Space - select, Enter - confirm)",
    copying: "Installing",
    backup_msg: "Backup created:",
    cleaning: "Cleaning cache...",
    done: "Installation completed successfully.",
    error_git: "Repository clone error.",
    dep_prompt: "Package '{}' not found. Install it via yay?",
};

fn main() -> Result<()> {
    let term = Term::stdout();
    term.clear_screen()?;

    let lang_idx = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Select Language")
        .items(&["RU", "EN"])
        .default(0)
        .interact()?;

    let l = if lang_idx == 0 { RU } else { EN };

    println!("\n{}", style(l.welcome).bold().cyan());
    println!("{}", style("—".repeat(40)).dim());

    let repo_url: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt(l.repo_prompt)
        .interact_text()?;

    let home = dirs::home_dir().context("Home directory not found")?;
    let temp_repo = home.join(".cache/dotmaster_temp");

    if temp_repo.exists() {
        fs::remove_dir_all(&temp_repo)?;
    }

    println!("\n{}", style(l.cloning).dim());
    let git_status = Command::new("git")
        .args(["clone", "--depth", "1", &repo_url, temp_repo.to_str().unwrap()])
        .status();

    if git_status.is_err() || !git_status.unwrap().success() {
        println!("{}", style(l.error_git).red());
        return Ok(());
    }

    // search
    let source_dir = find_config_dir(&temp_repo);

    let mode_idx = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt(l.mode_prompt)
        .items(&l.modes)
        .default(0)
        .interact()?;

    let target_root = if mode_idx == 0 {
        home.join(".config")
    } else {
        let raw_path: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt(l.path_prompt)
            .interact_text()?;
        if raw_path.starts_with("~/") {
            home.join(&raw_path[2..])
        } else {
            PathBuf::from(raw_path)
        }
    };

    // filter
    let entries: Vec<_> = fs::read_dir(&source_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            !name.starts_with('.') && 
            name != "readme.md" && 
            name != "license" && 
            name != "target" &&
            name != "pkgbuild"
        })
        .collect();

    let names: Vec<_> = entries
        .iter()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();

    let chosen_indices = MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt(l.select_prompt)
        .items(&names)
        .defaults(&vec![true; names.len()])
        .interact()?;

    for &idx in &chosen_indices {
        let name = &names[idx];
        let src = entries[idx].path();

        // hi!
        check_and_install_dep(name, &l)?;

        // install
        let dest = if src.is_file() { home.join(name) } else { target_root.join(name) };

        if dest.exists() && !dest.is_symlink() {
            backup_config(&dest, l.backup_msg)?;
        }

        if dest.exists() {
            if dest.is_dir() { fs::remove_dir_all(&dest)?; } else { fs::remove_file(&dest)?; }
        }

        // copying
        if src.is_dir() {
            copy_dir_all(&src, &dest)?;
        } else {
            fs::copy(&src, &dest)?;
        }
        
        println!("  {} {} -> {}", style("󰄬").green(), name, dest.display());
    }

    println!("\n{}", style(l.cleaning).dim());
    fs::remove_dir_all(&temp_repo)?;

    println!("\n{}", style(l.done).bold().green());
    Ok(())
}

fn find_config_dir(repo: &Path) -> PathBuf {
    let hints = ["config", ".config", "dotfiles", "dots"];
    for h in hints {
        let p = repo.join(h);
        if p.is_dir() { return p; }
    }
    
    let common = ["hypr", "waybar", "kitty", "nvim", "rofi", "fish", "zsh"];
    if let Ok(rd) = fs::read_dir(repo) {
        for e in rd.flatten() {
            if common.contains(&e.file_name().to_string_lossy().as_ref()) {
                return repo.to_path_buf();
            }
        }
    }
    repo.to_path_buf()
}

fn check_and_install_dep(name: &str, l: &Lang) -> Result<()> {
    // Список маппинга папок на пакеты (можно расширять)
    let pkg_name = match name {
        "hypr" => "hyprland-git",
        "waybar" => "waybar-hyprland",
        "nvim" => "neovim",
        "kitty" => "kitty",
        "rofi" => "rofi-lbonn-wayland-git",
        _ => return Ok(()),
    };

    // install 2
    if which(name).is_err() && Command::new("pacman").args(["-Qi", pkg_name]).output()?.status.success() == false {
        if Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(l.dep_prompt.replace("{}", pkg_name))
            .default(true)
            .interact()? 
        {
            Command::new("yay").args(["-S", "--noconfirm", pkg_name]).status()?;
        }
    }
    Ok(())
}

fn backup_config(path: &Path, msg: &str) -> Result<()> {
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let backup_dir = dirs::home_dir().unwrap().join(".dotmaster_backups").join(timestamp.to_string());
    fs::create_dir_all(&backup_dir)?;
    
    let dest = backup_dir.join(path.file_name().unwrap());
    Command::new("cp").args(["-r", path.to_str().unwrap(), dest.to_str().unwrap()]).status()?;
    println!("{} {:?}", style(msg).yellow(), dest);
    Ok(())
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
