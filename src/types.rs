#[derive(Debug, Copy, Clone)]
pub enum ItemType {
    // RFC 1436:
    File,
    Directory,
    CSO,
    Error,
    BinHex,
    DosBinary,
    Uuencoded,
    IndexSearch,
    Telnet,
    Binary,
    RedundantServer,
    Tn3270,
    Gif,
    Image,

    // Unofficial types:
    Document,
    HTML,
    Info,
    Audio,

    Reserved(u8),
    Other(u8),
}

impl ItemType {
    pub fn from_u8(c: u8) -> Self {
        #[allow(clippy::match_overlapping_arm)]
        match c {
            // RFC 1436:
            b'0' => Self::File,
            b'1' => Self::Directory,
            b'2' => Self::CSO,
            b'3' => Self::Error,
            b'4' => Self::BinHex,
            b'5' => Self::DosBinary,
            b'6' => Self::Uuencoded,
            b'7' => Self::IndexSearch,
            b'8' => Self::Telnet,
            b'9' => Self::Binary,
            b'+' => Self::RedundantServer,
            b'T' => Self::Tn3270,
            b'g' => Self::Gif,
            b'I' => Self::Image,

            // Unofficial:
            b'd' => Self::Document,
            b'h' => Self::HTML,
            b'i' => Self::Info,
            b's' => Self::Audio,

            #[allow(overlapping_patterns)]
            b'0' ..= b'Z' => Self::Reserved(c),

            _ => Self::Other(c),
        }
    }

    pub fn into_u8(self) -> u8 {
        match self {
            // RFC 1436:
            Self::File => b'0',
            Self::Directory => b'1',
            Self::CSO => b'2',
            Self::Error => b'3',
            Self::BinHex => b'4',
            Self::DosBinary => b'5',
            Self::Uuencoded => b'6',
            Self::IndexSearch => b'7',
            Self::Telnet => b'8',
            Self::Binary => b'9',
            Self::RedundantServer => b'+',
            Self::Tn3270 => b'T',
            Self::Gif => b'g',
            Self::Image => b'I',

            // Unofficial:
            Self::Document => b'd',
            Self::HTML => b'h',
            Self::Info => b'i',
            Self::Audio => b's',

            Self::Reserved(c) | Self::Other(c) => c,
        }
    }
}
