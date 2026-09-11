//! The `mazet` binary.
//!
//! Everything it does lives in the library ([`mazet`]); this is the
//! entrypoint, and it owns exactly two things: turning a parsed invocation
//! into printed bytes, and turning a failure into an exit code.

use std::process::ExitCode;

use clap::Parser;
use mazet::cli::{self, Cli, CommandError, Context};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let flags = cli.format.flags();

    let result = Context::discover().and_then(|ctx| cli::run(&cli, &ctx));

    match result {
        Ok(out) => {
            // Resolved against the output, not before it: `mazet hook <shell>`
            // and `mazet hook resolve` are read down a pipe by a shell that
            // wants the bytes, not TOON. See `CommandOutput::raw`.
            println!(
                "{}",
                cli::output::Format::resolve_for(flags, &out).render(&out)
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            // On stdout, and it is the one document this invocation printed.
            println!(
                "{}",
                cli::error_output(&err, cli::output::Format::resolve(flags))
            );
            exit_code(&err)
        }
    }
}

fn exit_code(err: &CommandError) -> ExitCode {
    u8::try_from(err.exit_code)
        .map(ExitCode::from)
        .unwrap_or(ExitCode::FAILURE)
}
