use anyhow::{Context, Result};
use chrono::Local;
use console::{style, Term};
use dialoguer::{theme::ColorfulTheme, FuzzySelect, Input, MultiSelect};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
}

const RU: Lang = Lang {
    welcome: "DOTMASTER // Системный установщик",
    repo_prompt: "URL репозитория",
    cloning: "Загрузка временных объектов...",
    mode_prompt: "Место установки",
    modes: ["Системный каталог (~/.config)", "Пользовательский путь"],
    path_prompt: "Укажите путь",
    select_prompt: "Выберите компоненты (Space - выбор, Enter - подтверждение)",
    copying: "Копирование",
    backup_msg: "Резервная копия:",
    cleaning: "Очистка кэша...",
    done: "Установка завершена успешно.",
    error_git: "Ошибка клонирования репозитория.",
};

const EN: Lang = Lang {
    welcome: "DOTMASTER // System Installer",
    repo_prompt: "Repository URL",
    cloning: "Downloading temporary objects...",
    mode_prompt: "Installation target",
    modes: ["System config (~/.config)", "Custom path"],
    path_prompt: "Specify path",
    select_prompt: "Select components (Space - select, Enter - confirm)",
    copying: "Copying",
    backup_msg: "Backup created:",
    cleaning: "Cleaning cache...",
    done: "Installation completed successfully.",
    error_git: "Repository clone error.",
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

    println!("\n{}", style(l.welcome).bold());
    println!("{}", style("—".repeat(30)).dim());

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

    let entries: Vec<_> = fs::read_dir(&source_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && !e.file_name().to_string_lossy().starts_with('.'))
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

    let pb = ProgressBar::new(chosen_indices.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.cyan} [{bar:30.white/black}] {pos}/{len} {msg}")?
            .progress_chars("=>-"),
    );

    for &idx in &chosen_indices {
        let name = &names[idx];
        let src = entries[idx].path();
        let dest = target_root.join(name);

        pb.set_message(format!("{}: {}", l.copying, name));

        if mode_idx == 0 && dest.exists() && !dest.is_symlink() {
            backup_config(&dest, l.backup_msg)?;
        }

        if dest.exists() {
            if dest.is_dir() {
                fs::remove_dir_all(&dest)?;
            } else {
                fs::remove_file(&dest)?;
            }
        }

        copy_dir_all(&src, &dest)?;
        pb.inc(1);
    }

    pb.finish_and_clear();
    
    println!("{}", style(l.cleaning).dim());
    fs::remove_dir_all(&temp_repo)?;

    println!("\n{}", style(l.done).bold().green());
    Ok(())
}

fn find_config_dir(repo: &Path) -> PathBuf {
    for v in ["config", ".config"] {
        let p = repo.join(v);
        if p.exists() && p.is_dir() {
            return p;
        }
    }
    repo.to_path_buf()
}

fn backup_config(path: &Path, msg: &str) -> Result<()> {
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let backup_root = dirs::home_dir().unwrap().join(".dotfiles_backup").join(timestamp.to_string());
    fs::create_dir_all(&backup_root)?;

    let name = path.file_name().unwrap();
    let dest = backup_root.join(name);

    Command::new("cp")
        .args(["-r", path.to_str().unwrap(), dest.to_str().unwrap()])
        .status()?;

    println!("\r{} {:?}", msg, backup_root);
    Ok(())
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}