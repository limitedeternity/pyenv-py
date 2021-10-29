use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::{self, Command, ExitStatus};
use std::{env, fs};

#[cfg(not(target_os = "windows"))]
fn with_exec_extension(binary: &Path) -> Vec<PathBuf> {
    vec![binary.into()]
}

#[cfg(target_os = "windows")]
fn with_exec_extension(binary: &Path) -> Vec<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    let w_binary = binary
        .as_os_str()
        .to_ascii_lowercase()
        .encode_wide()
        .collect::<Vec<_>>();

    env::var_os("PATHEXT")
        .and_then(|exts| {
            Some(
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
                    }),
            )
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
        .filter_map(|full_path| {
            if full_path.is_file() {
                Some(full_path)
            } else {
                None
            }
        })
        .next()
}

fn find_binary(binary: impl AsRef<Path>) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| find_binary_on_paths(binary, env::split_paths(&paths)))
}

#[cfg(not(target_os = "windows"))]
fn exit_with_child_status(status: ExitStatus) -> ! {
    use std::os::unix::process::ExitStatusExt;
    process::exit(status.code().unwrap_or(status.signal().unwrap_or(1)));
}

#[cfg(target_os = "windows")]
fn exit_with_child_status(status: ExitStatus) -> ! {
    process::exit(status.code().unwrap_or(1));
}

fn main() {
    let argv = env::args().collect::<Vec<_>>();
    let (argv_0, argv_rest) = argv.split_first().unwrap();
    let (py_ver, call_args) = argv_rest.split_first().unwrap_or_else(|| {
        eprintln!(
            "Usage: {} -{{version}}(-{{architecture}}) [arg1, arg2, ...]",
            argv_0
        );

        process::exit(1);
    });

    let version_dirs = fs::read_dir(
        find_binary("pyenv")
            .unwrap_or_else(|| {
                eprintln!("Unable to find pyenv");
                process::exit(1);
            })
            .ancestors()
            .skip(2)
            .next()
            .unwrap()
            .join("versions"),
    )
    .unwrap()
    .map(|entry| entry.unwrap().path())
    .collect::<Vec<_>>()
    .into_iter()
    .rev()
    .collect::<Vec<_>>();

    let avail_versions = version_dirs
        .iter()
        .map(|dir| dir.file_name().unwrap().to_string_lossy())
        .collect::<Vec<_>>();

    let ver_arg_regex = Regex::new(r"^(?:-(\d+(?:\.\d+){0,2}))(?:-(\d+))?$").unwrap();
    let captures = ver_arg_regex.captures(py_ver).map(|groups| {
        groups
            .iter() // All captured groups
            .skip(1) // Skip the complete match
            .flat_map(|c| c) // Ignore all empty optional matches
            .map(|c| c.as_str()) // Get original strings
            .collect::<Vec<_>>()
    });

    let version_idx_opt = match captures.as_ref().map(|c| c.as_slice()) {
        Some([py_ver]) => avail_versions.iter().position(|v| v.starts_with(py_ver)),

        Some([py_ver, "32"]) => match std::mem::size_of::<&char>() {
            4 => avail_versions.iter().position(|v| v.starts_with(py_ver)),

            8 => avail_versions
                .iter()
                .position(|v| v.starts_with(py_ver) && v.ends_with("win32")),

            _ => {
                eprintln!("Unable to determine CPU architecture");
                process::exit(1);
            }
        },

        Some([py_ver, "64"]) => match std::mem::size_of::<&char>() {
            4 => avail_versions
                .iter()
                .position(|v| v.starts_with(py_ver) && v.ends_with("amd64")),

            8 => avail_versions.iter().position(|v| v.starts_with(py_ver)),

            _ => {
                eprintln!("Unable to determine CPU architecture");
                process::exit(1);
            }
        },

        Some([_, _]) => {
            eprintln!("Invalid architecture specified");
            process::exit(1);
        }

        _ => {
            eprintln!("Unable to parse version string");
            process::exit(1);
        }
    };

    let python_binary = match version_idx_opt {
        Some(version_idx) => find_binary_on_paths(
            "python",
            vec![version_dirs[version_idx].to_owned()].into_iter(),
        )
        .unwrap_or_else(|| {
            eprintln!(
                "Python {} directory is damaged",
                avail_versions[version_idx]
            );

            process::exit(1);
        }),

        None => {
            eprintln!("Unable to find specified version");
            process::exit(1);
        }
    };

    let status = Command::new(python_binary)
        .args(call_args)
        .status()
        .expect("Unable to execute process");

    exit_with_child_status(status);
}
