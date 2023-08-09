//! POSIX errno

use core::fmt;
use num_enum::FromPrimitive;

/// Eerror number and message
#[repr(u32)]
#[derive(Copy, Clone, PartialEq, Eq, FromPrimitive, Default)]
pub enum Errno {
    #[default]
    /// ENo error (:
    None = 0,
    /// E2BIG (argument list too long)
    TooBig,
    /// EACCES (permission denied)
    PermissionDenied,
    /// EADDRINUSE (address in use)
    AddressInUse,
    /// EAFNOSUPPORT (address family not supported)
    AFNotSupported,
    /// EAGAIN (resource unavailable, try again)
    TryAgain,
    /// EALREADY (connection already in progress)
    ConnectionInProgress,
    /// EBADF (bad file descriptor)
    BadFile,
    /// EBADMSG (bad message)
    BadMessage,
    /// EBUSY (device or resource busy)
    Busy,
    /// ECANCELED (operation cancelled)
    Canceled,
    /// ECHILD (no child processes)
    NoChild,
    /// ECONNABORTED (connection aborted)
    ConnectionAborted,
    /// ECONNREFUSED (connection refused)
    ConnectionRefused,
    /// ECONNRESET (connection reset)
    ConnectionReset,
    /// EDEADLK (resource deadlock would occur)
    Deadlock,
    /// EDESTADDRREQ (destination address required)
    DestAddrRequired,
    /// EDOM (math argument out of domain of function)
    OutOfDomain,
    /// EQUOT (disk quota exceeded)
    DiskQuotaExceeded,
    /// EEXIST (file exists)
    Exists,
    /// EFAULT (bad address)
    BadAddress,
    /// EFBIG (file too big)
    FileTooBig,
    /// EHOSTUNREACH (host is unreachable)
    HostUnreachable,
    /// EIDRM (identifier removed)
    IdentifierRemoved,
    /// EILSEQ (illegal byte sequence)
    IllegalSequence,
    /// EINPROGRESS (operation in progress)
    InProgress,
    /// EINTR (interrupted function)
    Interrupted,
    /// EINVAL (invalid argument)
    InvalidArgument,
    /// EIO (input-output error)
    IOError,
    /// EISCONN (socket is connected)
    IsConnected,
    /// EISDIR (socket is directory)
    IsDirectory,
    /// ELOOP (too many levels of symbolic links)
    TooManySymLinks,
    /// EMFILE (file descriptor too big)
    FileDescTooBig,
    /// EMLINK (too many links)
    TooManyLinks,
    /// EMSGSIZE (message size too large)
    MessageTooLarge,
    /// EMULTIHOP (multihop attempted)
    MultihopAttempted,
    /// ENAMETOOLONG (filename too long)
    FilenameTooLong,
    /// ENETDOWN (network is down)
    NetworkDown,
    /// ENETRESET (connection aborted by network)
    NetworkReset,
    /// ENETUNREACH (network unreachable)
    NetworkUnreachable,
    /// ENFILE (too many files open in system)
    TooManyFilesOpen,
    /// ENOBUFS (no buffer space available)
    NoBufferSpace,
    /// ENODATA (no message available in queue)
    NoMessageAvailable,
    /// ENODEV (no such device)
    NoSuchDevice,
    /// ENOENT (no such file or directory)
    NoSuchFileOrDir,
    /// ENOEXEC (executable file format error)
    ExecutableFormatErr,
    /// ENOLCK (no locks available)
    NoLocksAvailable,
    /// ENOLINK (link has been severed)
    LinkSevered,
    /// ENOMEM (out of memory)
    OutOfMemory,
    /// ENOMSG (no message of the desired type)
    NoMessage,
    /// ENOPROTOOPT (protocol not available)
    ProtocolNotAvailable,
    /// ENOSPC (no space left on device)
    NoSpaceLeft,
    /// ENOSR (no stream resources)
    NoStreamResources,
    /// ENOSTR (not a stream)
    NotStream,
    /// ENOSYS (functionality not supported)
    FuncNotSupported,
    /// ENOTCONN (socket is not connected)
    SocketNotConnected,
    /// ENOTDIR (not a directory or a symbolic link to a directory)
    NotDirectory,
    /// ENOTEMPTY (directory not empty)
    DirectoryNotEmpty,
    /// ENOTRECOVERABLE (state not recoverable)
    StateNotRecoverable,
    /// ENOTSOCK (not a socket)
    NotSocket,
    /// ENOTSUP (not supported)
    NotSupported,
    /// ENOTTY (inappropriate I/O control operation)
    WrongIOControl,
    /// ENXIO (no such device or address)
    NoSuchDeviceOrAddress,
    /// EOPNOTSUPP (operation not supported on socket)
    OperationNotSupported,
    /// EOVERFLOW (value too large for data type)
    ValueOverflow,
    /// EOWNERDEAD (previous owner died)
    OwnerDied,
    /// EPERM (operation not permitted)
    OperationNotPermitted,
    /// EPIPE (broken pipe)
    BrokenPipe,
    /// EPROTO (protocol error)
    ProtocolError,
    /// EPROTONOSUPPORT (protocol not supported)
    ProtocolNotSupported,
    /// ERANGE (result too large)
    ResultTooLarge,
    /// EROFS (read-only filesystem)
    ReadOnlyFilesystem,
    /// ESPIPE (invalid seek)
    InvalidSeek,
    /// ESRCH (no such process)
    NoSuchProcess,
    /// ESTALE (stale NFS file handle)
    StaleHandle,
    /// ETIME (stream ioctl timeout)
    StreamControlTimeout,
    /// ETIMEDOUT (connection timed out)
    ConnectionTimedOut,
    /// ETXTBUSY (text file busy)
    TextFileBusy,
    /// EWOULDBLOCK (operation would block)
    OperationWouldBlock,
    /// EXDEV (cross-device link)
    CrossDeviceLink,
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
            Self::ReadOnlyFilesystem => "read-only filesystem",
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
        write!(f, "Errno {} ({})", *self as u32, self)
    }
}
