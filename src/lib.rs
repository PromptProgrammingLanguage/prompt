mod completion;
mod chat;
mod eleven_labs;
mod session;
mod image;
mod openai;
mod cohere;
mod config;
mod voice;

pub use config::{Config,JSONConfig,DEFAULT_CONFIG_FILE};
pub use completion::{CompletionOptions};
pub use session::{SessionCommand,SessionResult,SessionResultExt,SessionError};
pub use image::{
    ImageCommand,
    ImageResult,
    ImageError,
    ImageData,
    ImageUrl,
    ImageBinary,
    PictureSize,
    PictureFormat
};
pub use chat::{
    ChatCommand,
    ChatResult,
    ChatError,
    ChatMessage,
    ChatRole
};
pub use voice::{
    Voice,
    VoiceResult,
    VoiceError
};
