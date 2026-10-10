mod cli;
mod deveco;
mod guard;
mod model;
mod process;
mod server;
mod skill_setup;
mod transport;

fn main() {
    if let Err(error) = cli::main() {
        let args: Vec<_> = std::env::args().skip(1).take_while(|a| a != "--").collect();
        if args.iter().any(|a| a == "--json") {
            eprintln!(
                "{}",
                serde_json::json!({"error":format!("{error:#}"),"exit_code":125})
            );
        } else {
            eprintln!("omh: {error:#}");
        }
        // Hook errors must block rather than silently allow the pending command.
        std::process::exit(if args.first().is_some_and(|a| a == "hook") {
            2
        } else {
            125
        });
    }
}
