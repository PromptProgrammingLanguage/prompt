use std::sync::mpsc::channel;
use std::time::Duration;
use std::fs::{OpenOptions,File};
use std::io::{self,BufRead,BufReader,Write};
use std::path::PathBuf;
use tokio::sync::mpsc;
use notify::{Watcher, RecommendedWatcher, RecursiveMode, event::{Event,EventKind}};

pub async fn monitor(path: PathBuf) -> notify::Result<()> {
    let (tx, mut rx) = mpsc::channel(256);
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    let transcript = {
        match content.find("<->") {
            Some(divider_index) => content
                .clone()
                .split_off(divider_index + 4)
                .trim_start()
                .lines()
                .map(|line| format!("{line}\n\n"))
                .collect(),
            None => String::new()
        }
    };

    println!("");
    print!("{transcript}");
    io::stdout().flush().unwrap();

    let file_path = path.clone();
    let mut lines = content.lines().map(|s| s.to_string()).collect::<Vec<_>>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        match res {
            Ok(event) => {
                match event.kind {
                    EventKind::Modify(_) => {
                        let file = OpenOptions::new().read(true).open(&file_path).unwrap();
                        let reader = BufReader::new(file);
                        let appended = reader.lines()
                            .collect::<Result<Vec<String>, std::io::Error>>()
                            .unwrap()
                            .into_iter()
                            .skip(lines.len())
                            .collect::<Vec<_>>();

                        if let Err(e) = tx.blocking_send(appended.join("\n")) {
                            panic!("{:?}", e);
                        }

                        lines.extend(appended);
                    },
                    _ => (),
                }
           },
           Err(e) => println!("watch error: {:?}", e),
        }
    })?;

    OpenOptions::new().create(true).write(true).open(&path).expect("Could not open file");

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(&path, RecursiveMode::Recursive)?;

    while let Some(appended) = rx.recv().await {
        println!("{}", appended);
    }

    Ok(())
}
