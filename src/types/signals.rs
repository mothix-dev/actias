pub enum Signal {
    Hangup = 1,         // SIGHUP
    Interrupt,          // SIGINT
    Quit,               // SIGQUIT
    IllegalInstruction, // SIGILL
    Trap,               // SIGTRAP
    Abort,              // SIGABRT
    BusError,           // SIGBUS
    MathException,      // SIGFPE
    Kill,               // SIGKILL
    User1,              // SIGUSR1
    PageFault,          // SIGSEGV
    User2,              // SIGUSR2
    InvalidPipe,        // SIGPIPE
    Alarm,              // SIGALRM
    Terminate,          // SIGTERM
    ChildProcess,       // SIGCHLD
    Continue,           // SIGCONT
    Poll,               // SIGPOLL
    ProfileTimer,       // SIGPROF
    Stop,               // SIGSTOP
    BadSyscall,         // SIGSYS
    TerminalStop,       // SIGTSTP
    BackgroundRead,     // SIGTTIN
    BackgroundWrite,    // SIGTTOU
    OutOfBand,          // SIGURG
    VirtualTimer,       // SIGVTALRM
    CPUTime,            // SIGXCPU
    FileSizeLimit,      // SIGXFSZ
    WindowSizeChanged,  // SIGWINCH
}
