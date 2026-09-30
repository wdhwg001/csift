//! One line on stderr that can never abort the process.
//!
//! `eprintln!` PANICS when its write fails, exactly as `println!` does, and it fails in the
//! most ordinary pipe there is: `csift ... 2>&1 | head -c 1` hands stdout and stderr ONE
//! pipe, the reader leaves after a byte, the next stdout write fails and is reported through
//! `main`'s error arm - and that report is itself a write to the same closed pipe, so the
//! process aborted with 101 and a panic message where the surface had done everything right.
//! The same holds for a note a surface prints after its stream: with the reader gone it
//! aborts a run whose output already landed.
//!
//! [`stderr_line`] writes the line and IGNORES a failure, because when stderr cannot carry a
//! line there is no stream left to say so on: the reader that closed it is the reader the
//! line was for. That is not the silent failure the output contract forbids - a failure of
//! STDOUT still reaches `main` and still decides the exit code - and it is why the exit code
//! of a reporting surface is the same whether its stderr is open or shares the closed pipe.
//! A surface that writes its stdout with `println!` keeps the abort on its own stdout write;
//! this helper changes nothing about that.
//!
//! The line is formatted into ONE buffer and written with ONE `write_all`, because stderr is
//! unbuffered and a `write_fmt` there is a write per argument, which lets another process
//! writing to the same stream interleave into the middle of the line.

use std::io::Write;

/// Write `args` and a newline to stderr as one write; a write that fails is dropped.
pub fn stderr_line(args: std::fmt::Arguments<'_>) {
    let mut line = args.to_string();
    line.push('\n');
    // Nothing is left to report a failure on: see the module docs.
    let _ = std::io::stderr().lock().write_all(line.as_bytes());
}
