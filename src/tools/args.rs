use std::collections::HashMap;
use std::path::Path;

/// Parses `key=value` arguments.
pub fn parse_args<S: AsRef<str>>(args: &[S]) -> Result<HashMap<String, String>, String> {
    let mut parsed = HashMap::new();
    for arg in args {
        let arg = arg.as_ref();
        match arg.split('=').collect::<Vec<_>>().as_slice() {
            [key, value] => {
                parsed.insert((*key).to_string(), (*value).to_string());
            }
            _ => return Err(format!("Invalid argument {arg}")),
        }
    }
    Ok(parsed)
}

/// Reads `KEY=value` lines from the `.env` file in the working directory, on top of `args`.
pub fn read_env(args: HashMap<String, String>) -> HashMap<String, String> {
    read_env_file(Path::new("./.env"), args)
}

pub fn read_env_file(path: &Path, mut args: HashMap<String, String>) -> HashMap<String, String> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        println!("Failed to read .env file (maybe it doesn't exist?)");
        return args;
    };

    for line in contents.lines() {
        let stripped = line.trim_start();
        if stripped.starts_with('#') {
            continue;
        }
        match stripped.split_once('=') {
            Some((key, value)) => {
                args.insert(key.trim().to_string(), value.trim().to_string());
            }
            None if stripped.trim().is_empty() => {}
            None => println!("malformed line in .env: {line}"),
        }
    }
    args
}

/// The environment variables, overlaid with the `.env` file and then the arguments.
pub fn full_env(args: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    env.extend(read_env(args.clone()));
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_values() {
        let args = parse_args(&["games=5", "variant=No Variant"]).unwrap();
        assert_eq!(args["games"], "5");
        assert_eq!(args["variant"], "No Variant");
        assert!(parse_args(&["oops"]).is_err());
        assert!(parse_args(&["a=b=c"]).is_err());
    }
}
