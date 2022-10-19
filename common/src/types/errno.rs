//! POSIX errno

use core::fmt;
use num_enum::FromPrimitive;

/// error number and message
#[repr(u32)]
#[derive(Copy, Clone, PartialEq, Eq, FromPrimitive)]
pub enum Errno {
    #[num_enum(default)]
    None = 0, // No error (:
    TooBig,                // E2BIG
    PermissionDenied,      // EACCES
    AddressInUse,          // EADDRINUSE
    AFNotSupported,        // EAFNOSUPPORT
    TryAgain,              // EAGAIN
    ConnectionInProgress,  // EALREADY
    BadFile,               // EBADF
    BadMessage,            // EBADMSG
    Busy,                  // EBUSY
    Canceled,              // ECANCELED
    NoChild,               // ECHILD
    ConnectionAborted,     // ECONNABORTED
    ConnectionRefused,     // ECONNREFUSED
    ConnectionReset,       // ECONNRESET
    Deadlock,              // EDEADLK
    DestAddrRequired,      // EDESTADDRREQ
    OutOfDomain,           // EDOM (sadly there's no ESUB)
    DiskQuotaExceeded,     // EDQUOT
    Exists,                // EEXIST
    BadAddress,            // EFAULT
    FileTooBig,            // EFBIG
    HostUnreachable,       // EHOSTUNREACH
    IdentifierRemoved,     // EIDRM
    IllegalSequence,       // EILSEQ
    InProgress,            // EINPROGRESS
    Interrupted,           // EINTR
    InvalidArgument,       // EINVAL
    IOError,               // (EI) EIO
    IsConnected,           // EISCONN
    IsDirectory,           // EISDIR
    TooManySymLinks,       // ELOOP
    FileDescTooBig,        // EMFILE
    TooManyLinks,          // EMLINK
    MessageTooLarge,       // EMSGSIZE
    MultihopAttempted,     // EMULTIHOP
    FilenameTooLong,       // ENAMETOOLONG
    NetworkDown,           // ENETDOWN
    NetworkReset,          // ENETRESET
    NetworkUnreachable,    // ENETUNREACH
    TooManyFilesOpen,      // ENFILE
    NoBufferSpace,         // ENOBUFS
    NoMessageAvailable,    // ENODATA
    NoSuchDevice,          // ENODEV
    NoSuchFileOrDir,       // ENOENT
    ExecutableFormatErr,   // ENOEXEC
    NoLocksAvailable,      // ENOLCK
    LinkSevered,           // ENOLINK
    OutOfMemory,           // ENOMEM
    NoMessage,             // ENOMSG
    ProtocolNotAvailable,  // ENOPROTOOPT
    NoSpaceLeft,           // ENOSPC
    NoStreamResources,     // ENOSR
    NotStream,             // ENOSTR
    FuncNotSupported,      // ENOSYS
    SocketNotConnected,    // ENOTCONN
    NotDirectory,          // ENOTDIR
    DirectoryNotEmpty,     // ENOTEMPTY
    StateNotRecoverable,   // ENOTRECOVERABLE
    NotSocket,             // ENOTSOCK
    NotSupported,          // ENOTSUP
    WrongIOControl,        // ENOTTY
    NoSuchDeviceOrAddress, // ENXIO
    OperationNotSupported, // EOPNOTSUPP
    ValueOverflow,         // EOVERFLOW
    OwnerDied,             // EOWNERDEAD
    OperationNotPermitted, // EPERM
    BrokenPipe,            // EPIPE
    ProtocolError,         // EPROTO (GEN)
    ProtocolNotSupported,  // EPROTONOSUPPORT
    ResultTooLarge,        // ERANGE
    ReadOnlyFileSystem,    // EROFS
    InvalidSeek,           // ESPIPE
    NoSuchProcess,         // ESRCH
    StaleHandle,           // ESTALE
    StreamControlTimeout,  // ETIME
    ConnectionTimedOut,    // ETIMEDOUT
    TextFileBusy,          // ETXTBSY
    OperationWouldBlock,   // EWOULDBLOCK
    CrossDeviceLink,       // EXDEV
}

impl fmt::Display for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            Self::None => "no error",
            Self::TooBig => "argument list too long",
            Self::PermissionDenied => "permission denied",
            Self::AddressInUse => "address in use",
            Self::AFNotSupported => "address family not supported",
            Self::TryAgain => "resource unavailable, try again",
            Self::ConnectionInProgress => "connection already in progress",
            Self::BadFile => "bad file descriptor",
            Self::BadMessage => "bad message",
            Self::Busy => "device or resource busy",
            Self::Canceled => "operation cancelled",
            Self::NoChild => "no child processes",
            Self::ConnectionAborted => "connection aborted",
            Self::ConnectionRefused => "connection refused",
            Self::ConnectionReset => "connection reset",
            Self::Deadlock => "resource deadlock would occur",
            Self::DestAddrRequired => "destination address required",
            Self::OutOfDomain => "math argument out of domain of function",
            Self::DiskQuotaExceeded => "disk quota exceeded",
            Self::Exists => "file exists",
            Self::BadAddress => "bad address",
            Self::FileTooBig => "file too big",
            Self::HostUnreachable => "host is unreachable",
            Self::IdentifierRemoved => "identifier removed",
            Self::IllegalSequence => "illegal byte sequence",
            Self::InProgress => "operation in progress",
            Self::Interrupted => "interrupted function",
            Self::InvalidArgument => "invalid argument",
            Self::IOError => "input-output error",
            Self::IsConnected => "socket is connected",
            Self::IsDirectory => "socket is directory",
            Self::TooManySymLinks => "too many levels of symbolic links",
            Self::FileDescTooBig => "file descriptor too big",
            Self::TooManyLinks => "too many links",
            Self::MessageTooLarge => "message size too large",
            Self::MultihopAttempted => "multihop attempted",
            Self::FilenameTooLong => "filename too long",
            Self::NetworkDown => "network is down",
            Self::NetworkReset => "connection aborted by network",
            Self::NetworkUnreachable => "network unreachable",
            Self::TooManyFilesOpen => "too many files open in system",
            Self::NoBufferSpace => "no buffer space available",
            Self::NoMessageAvailable => "no message available in queue",
            Self::NoSuchDevice => "no such device",
            Self::NoSuchFileOrDir => "no such file or directory",
            Self::ExecutableFormatErr => "executable file format error",
            Self::NoLocksAvailable => "no locks available",
            Self::LinkSevered => "link has been severed",
            Self::OutOfMemory => "out of memory",
            Self::NoMessage => "no message of the desired type",
            Self::ProtocolNotAvailable => "protocol not available",
            Self::NoSpaceLeft => "no space left on device",
            Self::NoStreamResources => "no stream resources",
            Self::NotStream => "not a stream",
            Self::FuncNotSupported => "functionality not supported",
            Self::SocketNotConnected => "socket is not connected",
            Self::NotDirectory => "not a directory or a symbolic link to a directory",
            Self::DirectoryNotEmpty => "directory not empty",
            Self::StateNotRecoverable => "state not recoverable",
            Self::NotSocket => "not a socket",
            Self::NotSupported => "not supported",
            Self::WrongIOControl => "inappropriate I/O control operation",
            Self::NoSuchDeviceOrAddress => "no such device or address",
            Self::OperationNotSupported => "operation not supported on socket",
            Self::ValueOverflow => "value too large for data type",
            Self::OwnerDied => "previous owner died",
            Self::OperationNotPermitted => "operation not permitted",
            Self::BrokenPipe => "broken pipe",
            Self::ProtocolError => "protocol error",
            Self::ProtocolNotSupported => "protocol not supported",
            Self::ResultTooLarge => "result too large",
            Self::ReadOnlyFileSystem => "read-only file system",
            Self::InvalidSeek => "invalid seek",
            Self::NoSuchProcess => "no such process",
            Self::StaleHandle => "stale NFS file handle",
            Self::StreamControlTimeout => "stream ioctl timeout",
            Self::ConnectionTimedOut => "connection timed out",
            Self::TextFileBusy => "text file busy",
            Self::OperationWouldBlock => "operation would block",
            Self::CrossDeviceLink => "cross-device link",
        })
    }
}

impl fmt::Debug for Errno {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Errno: {}", self)
    }
}
