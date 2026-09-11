//! The shell hooks: shell code that keeps `AZURE_CONFIG_DIR` in step with the
//! directory, so bare `az` honours the tree with no wrapper.
//!
//! ```sh
//! eval "$(mazet hook bash)"     # ~/.bashrc
//! eval "$(mazet hook zsh)"      # ~/.zshrc
//! mazet hook fish | source      # ~/.config/fish/config.fish
//! Invoke-Expression (& mazet hook powershell | Out-String)   # $PROFILE
//! ```
//!
//! # The four properties
//!
//! **Fast.** One `mazet hook resolve` per prompt and nothing else: no `az`, no
//! network, no `jq`, and nothing that reads the store's contents. The answer
//! is one line on stdout and an exit code.
//!
//! **It unexports on the way out.** This is the one that does damage if it is
//! missing. Leaving a bound tree for an unbound directory restores whatever
//! `AZURE_CONFIG_DIR` the shell had before mazet took it over, and unsets it
//! when there was nothing. A hook that only ever *sets* the variable carries
//! the previous directory's identity into an unrelated one, and `az` then runs
//! against the wrong account with nothing on screen to say so. The same
//! applies when the `.mazet` fails to parse: an answer mazet could not compute
//! is never an answer to keep.
//!
//! **Idempotent.** Every script is wrapped in a one-shot guard and checks the
//! shell's own hook list before adding itself, so evaluating it twice in one
//! shell installs one hook.
//!
//! **Inert when it cannot work.** `mazet` missing from `PATH` leaves the shell
//! usable and the prompt working; a malformed `.mazet` is reported once, not
//! on every prompt, and the reporting resets as soon as the answer changes.
//!
//! Each hook also preserves the exit status of the command that ran before it,
//! so a prompt that shows `$?` keeps showing the truth.
//!
//! # The one call
//!
//! [`crate::cli::hook_cmd`] implements `mazet hook resolve`, whose contract is
//! narrow on purpose:
//!
//! | exit | stdout | the hook does |
//! |---|---|---|
//! | 0 | the store directory, one line | export it |
//! | 3 | an error document | restore what the shell had, silently |
//! | 1 | an error document | restore, and report once |
//! | 127 | (the shell's own) | restore, and report once |
//!
//! Nothing else about the resolution reaches the prompt: warnings, provenance
//! and the store's contents are `mazet which`'s job, not something to print
//! before every command line.

/// A shell `mazet hook` can emit code for.
// `PowerShell` reads as the enum's own name to clippy. It is the shell's
// name, spelled the way Microsoft spells it, and every other variant here is
// a shell's name too.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// GNU bash, through `PROMPT_COMMAND`.
    Bash,
    /// zsh, through `precmd_functions`.
    Zsh,
    /// fish, through a `PWD` variable handler.
    Fish,
    /// PowerShell, through the `prompt` function.
    PowerShell,
}

impl Shell {
    /// The name this shell is written as.
    pub fn as_str(self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::PowerShell => "powershell",
        }
    }

    /// The line to put in this shell's startup file.
    pub fn install_line(self) -> &'static str {
        match self {
            Shell::Bash => r#"eval "$(mazet hook bash)""#,
            Shell::Zsh => r#"eval "$(mazet hook zsh)""#,
            Shell::Fish => "mazet hook fish | source",
            Shell::PowerShell => "Invoke-Expression (& mazet hook powershell | Out-String)",
        }
    }

    /// Where that line goes.
    pub fn rc_file(self) -> &'static str {
        match self {
            Shell::Bash => "~/.bashrc",
            Shell::Zsh => "~/.zshrc",
            Shell::Fish => "~/.config/fish/config.fish",
            Shell::PowerShell => "$PROFILE",
        }
    }

    /// The shell code to evaluate.
    pub fn script(self) -> &'static str {
        match self {
            Shell::Bash => BASH,
            Shell::Zsh => ZSH,
            Shell::Fish => FISH,
            Shell::PowerShell => POWERSHELL,
        }
    }
}

const BASH: &str = r##"# mazet shell hook for bash.
#
#   eval "$(mazet hook bash)"        # in ~/.bashrc
#
# Re-resolves the current directory before every prompt and keeps
# AZURE_CONFIG_DIR in step with it -- including restoring what the shell had
# when you leave a bound tree. Evaluating this twice installs one hook.
if [ -z "${_MAZET_HOOK:-}" ]; then
  _MAZET_HOOK=1

  # Take AZURE_CONFIG_DIR over, remembering what was there the first time.
  _mazet_take() {
    if [ "${_MAZET_OWNED-}" != "${AZURE_CONFIG_DIR-}" ]; then
      if [ -n "${AZURE_CONFIG_DIR+set}" ]; then
        _MAZET_PREV_SET=1
        _MAZET_PREV=${AZURE_CONFIG_DIR}
      else
        _MAZET_PREV_SET=0
        _MAZET_PREV=
      fi
    fi
    AZURE_CONFIG_DIR=$1
    export AZURE_CONFIG_DIR
    _MAZET_OWNED=$1
  }

  # Give it back. A value the operator set themselves is left alone.
  _mazet_release() {
    if [ -n "${_MAZET_OWNED-}" ] && [ "${_MAZET_OWNED-}" = "${AZURE_CONFIG_DIR-}" ]; then
      if [ "${_MAZET_PREV_SET:-0}" = 1 ]; then
        AZURE_CONFIG_DIR=${_MAZET_PREV}
        export AZURE_CONFIG_DIR
      else
        unset AZURE_CONFIG_DIR
      fi
    fi
    unset _MAZET_OWNED _MAZET_PREV _MAZET_PREV_SET
  }

  _mazet_hook() {
    local _mazet_rc=$?
    local _mazet_out
    _mazet_out=$(command mazet hook resolve --text 2>/dev/null)
    local _mazet_status=$?
    if [ "$_mazet_status" -eq 0 ]; then
      _mazet_take "$_mazet_out"
      unset _MAZET_REPORTED
    else
      _mazet_release
      if [ "$_mazet_status" -eq 3 ]; then
        unset _MAZET_REPORTED
      elif [ "${_MAZET_REPORTED-}" != "$_mazet_status:$_mazet_out" ]; then
        _MAZET_REPORTED="$_mazet_status:$_mazet_out"
        if [ "$_mazet_status" -eq 127 ]; then
          printf '%s\n' "mazet: not found on PATH; the shell hook is idle." >&2
        else
          printf '%s\n' "mazet: $_mazet_out" >&2
        fi
      fi
    fi
    return $_mazet_rc
  }

  # bash 5.1 allows PROMPT_COMMAND to be an array; older bash has the string.
  case "$(declare -p PROMPT_COMMAND 2>/dev/null)" in
    "declare -a"*)
      PROMPT_COMMAND+=(_mazet_hook)
      ;;
    *)
      PROMPT_COMMAND="_mazet_hook${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
      ;;
  esac

  # The shell's own startup is a directory change like any other.
  _mazet_hook
fi
"##;

const ZSH: &str = r##"# mazet shell hook for zsh.
#
#   eval "$(mazet hook zsh)"         # in ~/.zshrc
#
# Re-resolves the current directory before every prompt and keeps
# AZURE_CONFIG_DIR in step with it -- including restoring what the shell had
# when you leave a bound tree. Evaluating this twice installs one hook.
if [ -z "${_MAZET_HOOK:-}" ]; then
  _MAZET_HOOK=1

  _mazet_take() {
    emulate -L zsh
    if [[ ${_MAZET_OWNED-} != "${AZURE_CONFIG_DIR-}" ]]; then
      if [[ -n ${AZURE_CONFIG_DIR+set} ]]; then
        _MAZET_PREV_SET=1
        _MAZET_PREV=$AZURE_CONFIG_DIR
      else
        _MAZET_PREV_SET=0
        _MAZET_PREV=
      fi
    fi
    export AZURE_CONFIG_DIR=$1
    _MAZET_OWNED=$1
  }

  _mazet_release() {
    emulate -L zsh
    if [[ -n ${_MAZET_OWNED-} && ${_MAZET_OWNED-} == "${AZURE_CONFIG_DIR-}" ]]; then
      if [[ ${_MAZET_PREV_SET:-0} == 1 ]]; then
        export AZURE_CONFIG_DIR=$_MAZET_PREV
      else
        unset AZURE_CONFIG_DIR
      fi
    fi
    unset _MAZET_OWNED _MAZET_PREV _MAZET_PREV_SET
  }

  _mazet_hook() {
    local _mazet_rc=$?
    emulate -L zsh
    local _mazet_out
    _mazet_out=$(command mazet hook resolve --text 2>/dev/null)
    local _mazet_status=$?
    if [[ $_mazet_status -eq 0 ]]; then
      _mazet_take "$_mazet_out"
      unset _MAZET_REPORTED
    else
      _mazet_release
      if [[ $_mazet_status -eq 3 ]]; then
        unset _MAZET_REPORTED
      elif [[ ${_MAZET_REPORTED-} != "$_mazet_status:$_mazet_out" ]]; then
        _MAZET_REPORTED="$_mazet_status:$_mazet_out"
        if [[ $_mazet_status -eq 127 ]]; then
          print -ru2 -- "mazet: not found on PATH; the shell hook is idle."
        else
          print -ru2 -- "mazet: $_mazet_out"
        fi
      fi
    fi
    return $_mazet_rc
  }

  typeset -ga precmd_functions
  if [[ -z ${precmd_functions[(r)_mazet_hook]} ]]; then
    precmd_functions+=(_mazet_hook)
  fi

  # The shell's own startup is a directory change like any other.
  _mazet_hook
fi
"##;

const FISH: &str = r##"# mazet shell hook for fish.
#
#   mazet hook fish | source         # in ~/.config/fish/config.fish
#
# Re-resolves the current directory whenever it changes and keeps
# AZURE_CONFIG_DIR in step with it -- including restoring what the shell had
# when you leave a bound tree. Sourcing this twice installs one hook.
if not set -q _MAZET_HOOK
    set -g _MAZET_HOOK 1

    function _mazet_take --argument-names store
        if test "$_MAZET_OWNED" != "$AZURE_CONFIG_DIR"
            if set -q AZURE_CONFIG_DIR
                set -g _MAZET_PREV_SET 1
                set -g _MAZET_PREV $AZURE_CONFIG_DIR
            else
                set -g _MAZET_PREV_SET 0
                set -g _MAZET_PREV ""
            end
        end
        set -gx AZURE_CONFIG_DIR $store
        set -g _MAZET_OWNED $store
    end

    function _mazet_release
        if set -q _MAZET_OWNED; and test "$_MAZET_OWNED" = "$AZURE_CONFIG_DIR"
            if test "$_MAZET_PREV_SET" = 1
                set -gx AZURE_CONFIG_DIR $_MAZET_PREV
            else
                set -e AZURE_CONFIG_DIR
            end
        end
        set -e _MAZET_OWNED
        set -e _MAZET_PREV
        set -e _MAZET_PREV_SET
    end

    function _mazet_hook --on-variable PWD
        set -l out ""
        command mazet hook resolve --text 2>/dev/null | read -lz out
        # $pipestatus is mazet's own status; `read` would have masked it.
        set -l rc $pipestatus[1]
        set out (string trim -- "$out")
        if test $rc -eq 0
            _mazet_take "$out"
            set -e _MAZET_REPORTED
        else
            _mazet_release
            if test $rc -eq 3
                set -e _MAZET_REPORTED
            else if test "$_MAZET_REPORTED" != "$rc:$out"
                set -g _MAZET_REPORTED "$rc:$out"
                if test $rc -eq 127
                    echo "mazet: not found on PATH; the shell hook is idle." >&2
                else
                    echo "mazet: $out" >&2
                end
            end
        end
    end

    # The shell's own startup is a directory change like any other.
    _mazet_hook
end
"##;

const POWERSHELL: &str = r##"# mazet shell hook for PowerShell.
#
#   Invoke-Expression (& mazet hook powershell | Out-String)   # in $PROFILE
#
# Re-resolves the current directory before every prompt and keeps
# AZURE_CONFIG_DIR in step with it -- including restoring what the shell had
# when you leave a bound tree. Evaluating this twice installs one hook.
if (-not (Get-Variable -Name MazetHook -Scope Global -ErrorAction SilentlyContinue)) {
    $global:MazetHook = 1
    $global:MazetOwned = $null
    $global:MazetPrev = $null
    $global:MazetPrevSet = $false
    $global:MazetReported = $null

    function global:MazetTake([string] $Store) {
        if ($global:MazetOwned -ne $env:AZURE_CONFIG_DIR) {
            if ($null -ne $env:AZURE_CONFIG_DIR) {
                $global:MazetPrevSet = $true
                $global:MazetPrev = $env:AZURE_CONFIG_DIR
            } else {
                $global:MazetPrevSet = $false
                $global:MazetPrev = $null
            }
        }
        $env:AZURE_CONFIG_DIR = $Store
        $global:MazetOwned = $Store
    }

    function global:MazetRelease {
        if ($null -ne $global:MazetOwned -and $global:MazetOwned -eq $env:AZURE_CONFIG_DIR) {
            if ($global:MazetPrevSet) {
                $env:AZURE_CONFIG_DIR = $global:MazetPrev
            } else {
                Remove-Item Env:AZURE_CONFIG_DIR -ErrorAction SilentlyContinue
            }
        }
        $global:MazetOwned = $null
        $global:MazetPrev = $null
        $global:MazetPrevSet = $false
    }

    function global:MazetHookRun {
        $previous = $global:LASTEXITCODE
        $out = ''
        $rc = 0
        try {
            $out = (& mazet hook resolve --text 2>$null | Out-String).Trim()
            $rc = $LASTEXITCODE
        } catch {
            $rc = 127
        }
        if ($rc -eq 0) {
            MazetTake $out
            $global:MazetReported = $null
        } else {
            MazetRelease
            if ($rc -eq 3) {
                $global:MazetReported = $null
            } elseif ($global:MazetReported -ne "${rc}:$out") {
                $global:MazetReported = "${rc}:$out"
                if ($rc -eq 127) {
                    [Console]::Error.WriteLine('mazet: not found on PATH; the shell hook is idle.')
                } else {
                    [Console]::Error.WriteLine("mazet: $out")
                }
            }
        }
        $global:LASTEXITCODE = $previous
    }

    $global:MazetPreviousPrompt = $function:prompt
    function global:prompt {
        MazetHookRun
        & $global:MazetPreviousPrompt
    }

    # The shell's own startup is a directory change like any other.
    MazetHookRun
}
"##;
