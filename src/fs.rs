use std::path::{Path, PathBuf};
use tokio::fs::{self, File};
use tokio::io;

pub use tokio::fs::{read_dir, DirEntry};

#[derive(Debug)]
pub enum FileType {
    Directory,
    Menu { file: File, path: PathBuf },
    File(File),
    NotFound,
}

fn map_not_found(r: io::Result<FileType>, not_found: FileType) -> io::Result<FileType> {
    match r {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(not_found),
        _ => r,
    }
}

pub async fn lookup(path: &Path) -> io::Result<FileType> {
    async fn inner(path: &Path) -> io::Result<FileType> {
        let meta = fs::metadata(path).await?;
        if meta.is_dir() {
            let menu_path = path.join("!menu");
            match File::open(&menu_path).await {
                Ok(file) => Ok(FileType::Menu { file, path: menu_path }),
                Err(e) => map_not_found(Err(e), FileType::Directory),
            }
        } else {
            Ok(FileType::File(File::open(path).await?))
        }
    }
    map_not_found(inner(path).await, FileType::NotFound)
}
