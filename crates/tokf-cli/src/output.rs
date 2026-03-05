/// Print a Serialize value as pretty JSON, logging errors to stderr.
pub fn print_json(value: &(impl serde::Serialize + ?Sized)) {
    match serde_json::to_string_pretty(value) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("[tokf] JSON serialization error: {e}"),
    }
}
