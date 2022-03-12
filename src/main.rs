mod op;
mod op7_metadata;

use op::{
    load_all_accounts, load_all_items, load_all_vaults, AccountDetails, ItemDetails, VaultDetails,
};
use op7_metadata::write_items;

use clap::Parser;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::Duration;
use std::{collections::HashMap, process::exit};

#[derive(Parser)]
struct Cli {
    /// The path to export the metadata files to. Defaults to ~/.config/op/bookmarks. For backwards compatibility with 1Password 7 use ~/Library/Containers/com.agilebits.onepassword7/Data/Library/Caches/Metadata/1Password
    #[clap(parse(from_os_str), short, long)]
    export_path: Option<PathBuf>,

    /// The path to the 1Password 8 database file to watch. Defaults to ~/Library/Group\ Containers/2BUA8C4S2C.com.1password/Library/Application\ Support/1Password/Data
    #[clap(parse(from_os_str), short, long)]
    watch_path: Option<PathBuf>,

    /// Account user UUIDs to generate metadata for. Leave empty to export bookmarks for all accounts. Use spaces to separate multiple accounts. UUIDs can be found using `op account list`.
    accounts: Vec<String>,
}

fn main() {
    let args = Cli::parse();
    if args.accounts.len() == 0 {
        println!("Will create bookmark metadata for all accounts...");
    } else {
        println!(
            "Will create bookmark metadata for account user uuids {:?}...",
            args.accounts
        );
    }

    let export_path = export_path(args.export_path);
    generate_opbookmarks(&args.accounts, &export_path);

    // Watch for changes
    if let Some(path) = args.watch_path {
        println!("Watching 1Password 8 data folder for changes ({:?})", path);
        if let Err(e) = watch(path, &args.accounts, &export_path) {
            println!("error: {:?}", e)
        }
    }
}

fn export_path(cli_path: Option<PathBuf>) -> PathBuf {
    if let Some(path) = cli_path {
        return path;
    }

    let mut path = dirs::home_dir().unwrap();
    path.push(".config/op/bookmarks");
    path
}

fn generate_opbookmarks(account_user_uuids: &Vec<String>, export_path: &std::path::PathBuf) {
    let accounts = load_all_accounts(account_user_uuids);

    if let Err(err) = accounts {
        eprintln!("Failed to load accounts: {:?}", err);
        exit(1);
    }

    let accounts = accounts.unwrap();
    let mut vaults_by_account: HashMap<AccountDetails, Vec<VaultDetails>> = HashMap::new();
    let mut items_by_vault: HashMap<VaultDetails, Vec<ItemDetails>> = HashMap::new();

    println!(
        "Exporting bookmarks for accounts {:?}",
        accounts
            .iter()
            .map(|a| a.id.clone())
            .collect::<Vec<String>>()
    );

    // Collect the vaults for each account
    for account in accounts.iter() {
        let vaults = load_all_vaults(&account.id);

        match vaults {
            Ok(vaults) => {
                vaults_by_account.insert((*account).clone(), vaults);
            }
            Err(err) => {
                eprintln!(
                    "Failed to load vaults for account {}: {:?}",
                    account.id, err
                );
            }
        }
    }

    // Collect the items for each vault
    for (account, vaults) in vaults_by_account.iter() {
        for vault in vaults.iter() {
            let items = load_all_items(&account.id, &vault.id);

            match items {
                Ok(items) => {
                    items_by_vault.insert((*vault).clone(), items);
                }
                Err(err) => {
                    eprintln!(
                        "Failed to load items for vault {} in account {}: {:?}",
                        vault.id, account.id, err
                    )
                }
            }
        }
    }

    // Write out metadata for each item
    for (account, vaults) in vaults_by_account.iter() {
        for vault in vaults.iter() {
            let items = items_by_vault.get(vault);

            match items {
                Some(items) => {
                    write_items(export_path, items, vault, account);
                }
                None => {
                    eprint!("Unexpected None for items in vault {}", vault.id);
                }
            }
        }
    }
    println!("Metadata files written to {:?}.", export_path);
}

fn watch(
    path: std::path::PathBuf,
    account_user_uuids: &Vec<String>,
    export_path: &std::path::PathBuf,
) -> notify::Result<()> {
    use notify::DebouncedEvent;
    let (tx, rx) = channel();

    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(5))?;

    watcher.watch(path, RecursiveMode::Recursive)?;

    loop {
        match rx.recv() {
            Ok(event) => match event {
                DebouncedEvent::NoticeRemove(path) => {
                    // SQLite removes the journal file after merging the contents with 1password.sqlite
                    if path.ends_with("1password.sqlite-journal") {
                        println!("1Password 8 data file changed. Updating metadata files...");
                        generate_opbookmarks(account_user_uuids, export_path);
                    } else {
                        println!("Ignoring NoticeRemove of {:?}", path);
                    }
                }
                _ => {
                    println!("Ignoring event {:?}", event)
                }
            },
            Err(e) => println!("watch error: {:?}", e),
        }
    }
}
