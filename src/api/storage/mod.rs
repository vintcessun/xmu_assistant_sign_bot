mod cold;
mod file;
mod hot;
mod image;
mod temp;
mod vector;

use crate::config::DATA_DIR as BASE_DATA_DIR;

pub use cold::ColdTable;
pub use file::File;
pub use file::FileBackend;
pub use file::FileStorage;
pub use hot::HotTable;
pub use image::ImageFile;
pub use temp::TempFile;
pub use vector::{HasEmbedding, VectorSearchEngine};

const BINCODE_CONFIG: bincode::config::Configuration<
    bincode::config::LittleEndian,
    bincode::config::Fixint,
> = bincode::config::standard().with_fixed_int_encoding();
