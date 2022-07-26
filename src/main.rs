use std::path::{Path, PathBuf};
use std::process::{self, Command, ExitStatus};
use std::{env, fs, iter};

use cfg_match::cfg_match;
use itertools::Itertools;
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    static ref PLACEHOLDER: String = "PLACEHOLDER".into();
    static ref PY_VER_REGEX: Regex = Regex::new(r"^(\d+)(?:\.(\d+))?(?:\.(\d+))?").unwrap();
    static ref PY_VER_ARG_REGEX: Regex =
        Regex::new(r"^(?:-(\d+(?:\.\d+){0,2}))(?:.*-(32|64))?").unwrap();
}

#[cfg(unix)]
fn with_exec_extension(binary: &Path) -> Vec<PathBuf> {
    vec![binary.into()]
}

#[cfg(windows)]
fn with_exec_extension(binary: &Path) -> Vec<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    let w_binary = binary
        .as_os_str()
        .to_ascii_lowercase()
        .encode_wide()
        .collect::<Vec<_>>();

    env::var_os("PATHEXT")
        .map(|exts| {
            env::split_paths(&exts)
                .filter_map(|ext| {
                    let w_ext = ext
                        .as_os_str()
                        .to_ascii_lowercase()
                        .encode_wide()
                        .collect::<Vec<_>>();

                    if w_binary.ends_with(&w_ext) {
                        Some(vec![binary.into()])
                    } else {
                        None
                    }
                })
                .next()
                .unwrap_or_else(|| {
                    env::split_paths(&exts)
                        .map(|ext| {
                            let mut with_ext = binary.as_os_str().to_owned();
                            with_ext.push(ext);
                            PathBuf::from(with_ext)
                        })
                        .collect::<Vec<_>>()
                })
        })
        .unwrap()
}

fn find_binary_on_paths(
    binary: impl AsRef<Path>,
    paths: impl Iterator<Item = PathBuf>,
) -> Option<PathBuf> {
    let binary_with_exec_exts = with_exec_extension(binary.as_ref());
    paths
        .flat_map(|dir| {
            binary_with_exec_exts
                .iter()
                .map(|ext_b| dir.join(ext_b))
                .collect::<Vec<_>>()
        })
        .find(|full_path| full_path.is_file())
}

fn find_binary(binary: impl AsRef<Path>) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| find_binary_on_paths(binary, env::split_paths(&paths)))
}

#[cfg(unix)]
fn exit_with_child_status(status: ExitStatus) -> ! {
    use std::os::unix::process::ExitStatusExt;
    process::exit(
        status
            .code()
            .unwrap_or_else(|| status.signal().unwrap_or(1)),
    );
}

#[cfg(windows)]
fn exit_with_child_status(status: ExitStatus) -> ! {
    process::exit(status.code().unwrap_or(1));
}

fn main() {
    let argv = env::args().collect::<Vec<_>>();
    let (_, argv_rest) = argv.split_first().unwrap();
    let (py_ver_arg, py_call_args) = argv_rest
        .split_first()
        .and_then(|pair| {
            if PY_VER_ARG_REGEX.is_match(pair.0) {
                Some(pair)
            } else {
                None
            }
        })
        .unwrap_or((&PLACEHOLDER, argv_rest));

    let pyenv_binary = find_binary("pyenv").unwrap_or_else(|| {
        eprintln!("Unable to find pyenv");
        process::exit(1);
    });

    let versions_dir = pyenv_binary.ancestors().nth(2).unwrap().join("versions");
    let shims_dir = pyenv_binary.ancestors().nth(2).unwrap().join("shims");

    let (py_bin_locs, loc_dirnames) = fs::read_dir(versions_dir)
        .unwrap()
        .filter_map(|result| match result {
            Ok(entry) => Some(entry.path()),
            _ => None,
        })
        .chain(iter::once(shims_dir))
        .map(|path| {
            let dirname = path.file_name().unwrap().to_string_lossy().to_string();
            let captures = PY_VER_REGEX.captures(&dirname).map(|groups| {
                groups
                    .iter()
                    .skip(1)
                    .flatten()
                    .map(|c| c.as_str().parse::<u32>().unwrap())
                    .collect::<Vec<_>>()
            });

            (
                match captures.as_deref() {
                    Some([major, minor, patch]) => (*major, *minor, *patch),
                    Some([major, minor]) => (*major, *minor, 0),
                    Some([major]) => (*major, 0, 0),
                    _ => (0, 0, 0),
                },
                (path, dirname),
            )
        })
        .sorted_by_key(|pair| pair.0)
        .rev()
        .map(|pair| pair.1)
        .unzip::<_, _, Vec<_>, Vec<_>>();

    let captures = PY_VER_ARG_REGEX.captures(py_ver_arg).map(|groups| {
        groups
            .iter()
            .skip(1)
            .flatten()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
    });

    let version_idx_opt = match captures.as_deref() {
        Some([py_ver]) => loc_dirnames.iter().position(|v| v.starts_with(py_ver)),

        Some([py_ver, "32"]) => cfg_match! {
            target_pointer_width = "32" => loc_dirnames.iter().position(|v| v.starts_with(py_ver) && !v.ends_with("amd64")),

            _ => loc_dirnames
                .iter()
                .position(|v| v.starts_with(py_ver) && v.ends_with("win32"))
        },

        Some([py_ver, "64"]) => cfg_match! {
            target_pointer_width = "64" => loc_dirnames.iter().position(|v| v.starts_with(py_ver) && !v.ends_with("win32")),

            _ => loc_dirnames
                .iter()
                .position(|v| v.starts_with(py_ver) && v.ends_with("amd64"))
        },

        _ => py_bin_locs
            .iter()
            .position(|v| v.components().nth_back(1).unwrap().as_os_str() != "versions"),
    };

    let python_binary = match version_idx_opt {
        Some(version_idx) => find_binary_on_paths(
            "python",
            vec![py_bin_locs[version_idx].to_owned()].into_iter(),
        )
        .unwrap_or_else(|| {
            eprintln!("Unable to find python binary");
            process::exit(1);
        }),

        _ => {
            eprintln!("Unable to find specified version");
            process::exit(1);
        }
    };

    ctrlc::set_handler(move || {}).expect("Unable to set Ctrl-C handler");

    let status = Command::new(python_binary)
        .args(py_call_args)
        .status()
        .expect("Unable to execute the binary");

    exit_with_child_status(status);
}
