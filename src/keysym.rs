use core::fmt;

/// key symbols, just copied from linux tbh
/// did i put basically every keysym ever in an enum? you bet!
#[repr(u16)]
#[derive(Debug, Copy, Clone)]
pub enum KeySym {
    Null = 0,
    /// ctrl + a
    CtrlA,
    /// ctrl + b
    CtrlB,
    /// ctrl + c
    CtrlC,
    /// ctrl + d
    CtrlD,
    /// ctrl + e
    CtrlE,
    /// ctrl + f
    CtrlF,
    /// ctrl + g
    CtrlG,
    /// backspace / ctrl + h
    Backspace,
    /// tab / ctrl + i
    Tab,
    /// linefeed / ctrl + j
    Linefeed,
    /// ctrl + k
    CtrlK,
    /// ctrl + l
    CtrlL,
    /// ctrl + m
    CtrlM,
    /// ctrl + n
    CtrlN,
    /// ctrl + o
    CtrlO,
    /// ctrl + p
    CtrlP,
    /// ctrl + q
    CtrlQ,
    /// ctrl + r
    CtrlR,
    /// ctrl + s
    CtrlS,
    /// ctrl + t
    CtrlT,
    /// ctrl + u
    CtrlU,
    /// ctrl + v
    CtrlV,
    /// ctrl + w
    CtrlW,
    /// ctrl + x
    CtrlX,
    /// ctrl + y
    CtrlY,
    /// ctrl + z
    CtrlZ,
    /// escape
    Escape,
    /// ctrl + \
    CtrlBackslash,
    /// ctrl + [
    CtrlBracketRight,
    /// ctrl + ^
    CtrlCircumflex,
    /// ctrl + _
    CtrlUnderscore,
    /// space ( )
    Space,
    /// !
    Exclam,
    /// "
    DoubleQuote,
    /// #
    NumberSign,
    /// $
    Dollar,
    /// %
    Percent,
    /// &
    Ampersand,
    /// '
    Apostrophe,
    /// (
    ParenLeft,
    /// )
    ParenRight,
    /// *
    Asterisk,
    /// +
    Plus,
    /// ,
    Comma,
    /// -
    Minus,
    /// .
    Period,
    /// /
    Slash,
    /// 0
    Zero,
    /// 1
    One,
    /// 2
    Two,
    /// 3
    Three,
    /// 4
    Four,
    /// 5
    Five,
    /// 6
    Six,
    /// 7
    Seven,
    /// 8
    Eight,
    /// 9
    Nine,
    /// :
    Colon,
    /// ;
    Semicolon,
    /// <
    Less,
    /// =
    Equal,
    /// >
    Greater,
    /// ?
    Question,
    /// @
    At,
    /// A
    UpperA,
    /// B
    UpperB,
    /// C
    UpperC,
    /// D
    UpperD,
    /// E
    UpperE,
    /// F
    UpperF,
    /// G
    UpperG,
    /// H
    UpperH,
    /// I
    UpperI,
    /// J
    UpperJ,
    /// K
    UpperK,
    /// L
    UpperL,
    /// M
    UpperM,
    /// N
    UpperN,
    /// O
    UpperO,
    /// P
    UpperP,
    /// Q
    UpperQ,
    /// R
    UpperR,
    /// S
    UpperS,
    /// T
    UpperT,
    /// U
    UpperU,
    /// V
    UpperV,
    /// W
    UpperW,
    /// X
    UpperX,
    /// Y
    UpperY,
    /// Z
    UpperZ,
    /// [
    BracketLeft,
    /// \
    Backslash,
    /// ]
    BracketRight,
    /// ^
    Circumflex,
    /// _
    Underscore,
    /// `
    Grave,
    /// a
    LowerA,
    /// b
    LowerB,
    /// c
    LowerC,
    /// d
    LowerD,
    /// e
    LowerE,
    /// f
    LowerF,
    /// g
    LowerG,
    /// h
    LowerH,
    /// i
    LowerI,
    /// j
    LowerJ,
    /// k
    LowerK,
    /// l
    LowerL,
    /// m
    LowerM,
    /// n
    LowerN,
    /// o
    LowerO,
    /// p
    LowerP,
    /// q
    LowerQ,
    /// r
    LowerR,
    /// s
    LowerS,
    /// t
    LowerT,
    /// u
    LowerU,
    /// v
    LowerV,
    /// w
    LowerW,
    /// x
    LowerX,
    /// y
    LowerY,
    /// z
    LowerZ,
    /// {
    BraceLeft,
    /// |
    Bar,
    /// }
    BraceRight,
    /// ~
    Tilde,
    /// delete key
    Delete,
    /// non-breaking space
    NonBreakingSpace,
    /// ¡
    ExclamDown,
    /// ¢
    Cent,
    /// £
    Pound,
    /// ¤
    Currency,
    /// ¥
    Yen,
    /// ¦
    BrokenBar,
    /// §
    Section,
    /// ¨
    Dieresis,
    /// ©
    Copyright,
    /// ª
    OrdFeminine,
    /// «
    GuillemotLeft,
    /// ¬
    NotSign,
    /// -
    Hyphen,
    /// ®
    Registered,
    /// ¯
    Macron,
    /// °
    Degree,
    /// ±
    PlusMinus,
    /// ²
    TwoSuperior,
    /// ³
    ThreeSuperior,
    /// ´
    Acute,
    /// µ
    Mu,
    /// ¶
    Paragraph,
    /// ·
    PeriodCentered,
    /// ¸
    Cedilla,
    /// ¹
    OneSuperior,
    /// º
    Masculine,
    /// »
    GuillemotRight,
    /// ¼
    OneQuarter,
    /// ½
    OneHalf,
    /// ¾
    ThreeQuarters,
    /// ¿
    QuestionDown,
    /// À
    AGrave,
    /// Á
    AAcute,
    /// Â
    ACircumflex,
    /// Ã
    ATilde,
    /// Ä
    ADieresis,
    /// Å
    ARing,
    /// Æ
    AE,
    /// Ç
    CCedilla,
    /// È
    EGrave,
    /// É
    EAcute,
    /// Ê
    ECircumflex,
    /// Ë
    EDieresis,
    /// Ì
    IGrave,
    /// Í
    IAcute,
    /// Î
    ICircumflex,
    /// Ï
    IDieresis,
    /// Ð
    Eth,
    /// Ñ
    NTilde,
    /// Ò
    OGrave,
    /// Ó
    OAcute,
    /// Ô
    OCircumflex,
    /// Õ
    OTilde,
    /// Ö
    ODieresis,
    /// ×
    Multiply,
    /// Ø
    OSlash,
    /// Ù
    UGrave,
    /// Ú
    UAcute,
    /// Û
    UCircumflex,
    /// Ü
    UDieresis,
    /// Ý
    YAcute,
    /// Þ
    Thorn,
    /// ß
    SSharp,
    /// à
    LowerAGrave,
    /// á
    LowerAAcute,
    /// â
    LowerACircumflex,
    /// ã
    LowerATilde,
    /// ä
    LowerADieresis,
    /// å
    LowerARing,
    /// æ
    LowerAE,
    /// ç
    LowerCCedilla,
    /// è
    LowerEGrave,
    /// é
    LowerEAcute,
    /// ê
    LowerECircumflex,
    /// ë
    LowerEDieresis,
    /// ì
    LowerIGrave,
    /// í
    LowerIAcute,
    /// î
    LowerICircumflex,
    /// ï
    LowerIDieresis,
    /// ð
    LowerEth,
    /// ñ
    LowerNTilde,
    /// ò
    LowerOGrave,
    /// ó
    LowerOAcute,
    /// ô
    LowerOCircumflex,
    /// õ
    LowerOTilde,
    /// ö
    LowerODieresis,
    /// ÷
    Division,
    /// ø
    LowerOSlash,
    /// ù
    LowerUGrave,
    /// ú
    LowerUAcute,
    /// û
    LowerUCircumflex,
    /// ü
    LowerUDieresis,
    /// ý
    LowerYAcute,
    /// þ
    LowerThorn,
    /// ÿ
    LowerYDieresis,
    /// f1 key
    F1,
    /// f2 key
    F2,
    /// f3 key
    F3,
    /// f4 key
    F4,
    /// f5 key
    F5,
    /// f6 key
    F6,
    /// f7 key
    F7,
    /// f8 key
    F8,
    /// f9 key
    F9,
    /// f10 key
    F10,
    /// f11 key
    F11,
    /// f12 key
    F12,
    /// f13 key
    F13,
    /// f14 key
    F14,
    /// f15 key
    F15,
    /// f16 key
    F16,
    /// f17 key
    F17,
    /// f18 key
    F18,
    /// f19 key
    F19,
    /// f20 key
    F20,
    /// home key
    Home,
    /// insert key
    Insert,
    /// remove key
    Remove,
    /// end key
    End,
    /// page up key
    PageUp,
    /// page down key
    PageDown,
    /// macro key?
    Macro,
    /// help key
    Help,
    /// do key? what the fuck
    Do,
    /// pause key
    Pause,
    // honestly dont give a shit anymore
    F21,
    F22,
    F23,
    F24,
    F25,
    F26,
    F27,
    F28,
    F29,
    F30,
    F31,
    F32,
    F33,
    F34,
    F35,
    F36,
    F37,
    F38,
    F39,
    F40,
    F41,
    F42,
    F43,
    F44,
    F45,
    F46,
    F47,
    F48,
    F49,
    F50,
    F51,
    F52,
    F53,
    F54,
    F55,
    F56,
    F57,
    F58,
    F59,
    F60,
    F61,
    F62,
    F63,
    F64,
    F65,
    F66,
    F67,
    F68,
    F69,
    F70,
    F71,
    F72,
    F73,
    F74,
    F75,
    F76,
    F77,
    F78,
    F79,
    F80,
    F81,
    F82,
    F83,
    F84,
    F85,
    F86,
    F87,
    F88,
    F89,
    F90,
    F91,
    F92,
    F93,
    F94,
    F95,
    F96,
    F97,
    F98,
    F99,
    F100,
    F101,
    F102,
    F103,
    F104,
    F105,
    F106,
    F107,
    F108,
    F109,
    F110,
    F111,
    F112,
    F113,
    F114,
    F115,
    F116,
    F117,
    F118,
    F119,
    F120,
    F121,
    F122,
    F123,
    F124,
    F125,
    F126,
    F127,
    F128,
    F129,
    F130,
    F131,
    F132,
    F133,
    F134,
    F135,
    F136,
    F137,
    F138,
    F139,
    F140,
    F141,
    F142,
    F143,
    F144,
    F145,
    F146,
    F147,
    F148,
    F149,
    F150,
    F151,
    F152,
    F153,
    F154,
    F155,
    F156,
    F157,
    F158,
    F159,
    F160,
    F161,
    F162,
    F163,
    F164,
    F165,
    F166,
    F167,
    F168,
    F169,
    F170,
    F171,
    F172,
    F173,
    F174,
    F175,
    F176,
    F177,
    F178,
    F179,
    F180,
    F181,
    F182,
    F183,
    F184,
    F185,
    F186,
    F187,
    F188,
    F189,
    F190,
    F191,
    F192,
    F193,
    F194,
    F195,
    F196,
    F197,
    F198,
    F199,
    F200,
    F201,
    F202,
    F203,
    F204,
    F205,
    F206,
    F207,
    F208,
    F209,
    F210,
    F211,
    F212,
    F213,
    F214,
    F215,
    F216,
    F217,
    F218,
    F219,
    F220,
    F221,
    F222,
    F223,
    F224,
    F225,
    F226,
    F227,
    F228,
    F229,
    F230,
    F231,
    F232,
    F233,
    F234,
    F235,
    F236,
    F237,
    F238,
    F239,
    F240,
    F241,
    F242,
    F243,
    F244,
    F245,
    /// why the FUCK are there 246 F keys???
    F246,

    // ton of keys after this that i just dont give a shit about. so they're not here

    /// does Absolutely Nothing!
    VoidSymbol = 0x0200,
    /// return key
    Return,

    /// break key
    Break = 0x0205,
    /// move back to last console
    LastConsole,
    /// caps lock key
    CapsLock,
    /// num lock key
    NumLock,
    /// scroll lock key
    ScrollLock,

    /// move down a tty
    DecrConsole = 0x0210,
    /// move up a tty
    IncrConsole,

    /// 0 on keypad
    KP0 = 0x0300,
    /// 1 on keypad
    KP1,
    /// 2 on keypad
    KP2,
    /// 3 on keypad
    KP3,
    /// 4 on keypad
    KP4,
    /// 5 on keypad
    KP5,
    /// 6 on keypad
    KP6,
    /// 7 on keypad
    KP7,
    /// 8 on keypad
    KP8,
    /// 9 on keypad
    KP9,
    /// + on keypad
    KPAdd,
    /// - on keypad
    KPSubtract,
    /// * on keypad
    KPMultiply,
    /// / on keypad
    KPDivide,
    /// enter on keypad
    KPEnter,
    /// , on keypad
    KPComma,
    /// . on keypad
    KPPeriod,
    /// whatever the hell that combined minus and plus symbol is on keypad
    KPMinPlus,

    /// down arrow key
    Down = 0x0600,
    /// left arrow key
    Left,
    /// right arrow key
    Right,
    /// up arrow key
    Up,

    /// shift key
    Shift = 0x0700,
    /// right alt key aka altgr
    AltGr,
    /// control key
    Ctrl,
    /// left alt key
    Alt,
    /// left shift key
    LeftShift,
    /// right shift key
    RightShift,
    /// left control key
    LeftCtrl,
    /// right control key
    RightCtrl,

    MetaNull = 0x800,
    /// meta + ctrl + a
    MetaCtrlA,
    /// meta + ctrl + b
    MetaCtrlB,
    /// meta + ctrl + c
    MetaCtrlC,
    /// meta + ctrl + d
    MetaCtrlD,
    /// meta + ctrl + e
    MetaCtrlE,
    /// meta + ctrl + f
    MetaCtrlF,
    /// meta + ctrl + g
    MetaCtrlG,
    /// meta + backspace / meta + ctrl + h
    MetaBackspace,
    /// meta + tab / meta + ctrl + i
    MetaTab,
    /// meta + linefeed / meta + ctrl + j
    MetaLinefeed,
    /// meta + ctrl + k
    MetaCtrlK,
    /// meta + ctrl + l
    MetaCtrlL,
    /// meta + ctrl + m
    MetaCtrlM,
    /// meta + ctrl + n
    MetaCtrlN,
    /// meta + ctrl + o
    MetaCtrlO,
    /// meta + ctrl + p
    MetaCtrlP,
    /// meta + ctrl + q
    MetaCtrlQ,
    /// meta + ctrl + r
    MetaCtrlR,
    /// meta + ctrl + s
    MetaCtrlS,
    /// meta + ctrl + t
    MetaCtrlT,
    /// meta + ctrl + u
    MetaCtrlU,
    /// meta + ctrl + v
    MetaCtrlV,
    /// meta + ctrl + w
    MetaCtrlW,
    /// meta + ctrl + x
    MetaCtrlX,
    /// meta + ctrl + y
    MetaCtrlY,
    /// meta + ctrl + z
    MetaCtrlZ,
    /// meta + escape
    MetaEscape,
    /// meta + ctrl + \
    MetaCtrlBackslash,
    /// meta + ctrl + [
    MetaCtrlBracketRight,
    /// meta + ctrl + ^
    MetaCtrlCircumflex,
    /// meta + ctrl + _
    MetaCtrlUnderscore,
    /// meta + space
    MetaSpace,
    /// meta + !
    MetaExclam,
    /// meta + "
    MetaDoubleQuote,
    /// meta + #
    MetaNumberSign,
    /// meta + $
    MetaDollar,
    /// meta + %
    MetaPercent,
    /// meta + &
    MetaAmpersand,
    /// meta + '
    MetaApostrophe,
    /// meta + (
    MetaParenLeft,
    /// meta + )
    MetaParenRight,
    /// meta + *
    MetaAsterisk,
    /// meta + +
    MetaPlus,
    /// meta + ,
    MetaComma,
    /// meta + -
    MetaMinus,
    /// meta + .
    MetaPeriod,
    /// meta + /
    MetaSlash,
    /// meta + 0
    MetaZero,
    /// meta + 1
    MetaOne,
    /// meta + 2
    MetaTwo,
    /// meta + 3
    MetaThree,
    /// meta + 4
    MetaFour,
    /// meta + 5
    MetaFive,
    /// meta + 6
    MetaSix,
    /// meta + 7
    MetaSeven,
    /// meta + 8
    MetaEight,
    /// meta + 9
    MetaNine,
    /// meta + :
    MetaColon,
    /// meta + ;
    MetaSemicolon,
    /// meta + <
    MetaLess,
    /// meta + =
    MetaEqual,
    /// meta + >
    MetaGreater,
    /// meta + ?
    MetaQuestion,
    /// meta + @
    MetaAt,
    /// meta + shift + a
    MetaShiftA,
    /// meta + shift + b
    MetaShiftB,
    /// meta + shift + c
    MetaShiftC,
    /// meta + shift + d
    MetaShiftD,
    /// meta + shift + e
    MetaShiftE,
    /// meta + shift + f
    MetaShiftF,
    /// meta + shift + g
    MetaShiftG,
    /// meta + shift + h
    MetaShiftH,
    /// meta + shift + i
    MetaShiftI,
    /// meta + shift + j
    MetaShiftJ,
    /// meta + shift + k
    MetaShiftK,
    /// meta + shift + l
    MetaShiftL,
    /// meta + shift + m
    MetaShiftM,
    /// meta + shift + n
    MetaShiftN,
    /// meta + shift + o
    MetaShiftO,
    /// meta + shift + p
    MetaShiftP,
    /// meta + shift + q
    MetaShiftQ,
    /// meta + shift + r
    MetaShiftR,
    /// meta + shift + s
    MetaShiftS,
    /// meta + shift + t
    MetaShiftT,
    /// meta + shift + u
    MetaShiftU,
    /// meta + shift + v
    MetaShiftV,
    /// meta + shift + w
    MetaShiftW,
    /// meta + shift + x
    MetaShiftX,
    /// meta + shift + y
    MetaShiftY,
    /// meta + shift + z
    MetaShiftZ,
    /// meta + [
    MetaBracketLeft,
    /// meta + \
    MetaBackslash,
    /// meta + ]
    MetaBracketRight,
    /// meta + ^
    MetaCircumflex,
    /// meta + _
    MetaUnderscore,
    /// meta + `
    MetaGrave,
    /// meta + a
    MetaA,
    /// meta + b
    MetaB,
    /// meta + c
    MetaC,
    /// meta + d
    MetaD,
    /// meta + e
    MetaE,
    /// meta + f
    MetaF,
    /// meta + g
    MetaG,
    /// meta + h
    MetaH,
    /// meta + i
    MetaI,
    /// meta + j
    MetaJ,
    /// meta + k
    MetaK,
    /// meta + l
    MetaL,
    /// meta + m
    MetaM,
    /// meta + n
    MetaN,
    /// meta + o
    MetaO,
    /// meta + p
    MetaP,
    /// meta + q
    MetaQ,
    /// meta + r
    MetaR,
    /// meta + s
    MetaS,
    /// meta + t
    MetaT,
    /// meta + u
    MetaU,
    /// meta + v
    MetaV,
    /// meta + w
    MetaW,
    /// meta + x
    MetaX,
    /// meta + y
    MetaY,
    /// meta + z
    MetaZ,
    /// meta + {
    MetaBraceLeft,
    /// meta + |
    MetaBar,
    /// meta + }
    MetaBraceRight,
    /// meta + ~
    MetaTilde,
    /// meta + delete key
    MetaDelete,

    /// shift lock (toggles shift)
    ShiftLock = 0x0a00,
    /// altgr lock (toggles altgr)
    AltGrLock,
    /// ctrl lock (toggles ctrl)
    CtrlLock,
    /// alt lock (toggles alt)
    AltLock,
    /// left shift lock (toggles left shift)
    LeftShiftLock,
    /// right shift lock (toggles right shift)
    RightShiftLock,
    /// left ctrl lock (toggles left ctrl)
    LeftCtrlLock,
    /// right ctrl lock (toggles right ctrl)
    RightCtrlLock,
}

impl fmt::Display for KeySym {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let num = *self as u16;
        if num < 0x0100 {
            write!(f, "{}", num as u8 as char)
        } else {
            write!(f, "{:?}", self)
        }
    }
}
