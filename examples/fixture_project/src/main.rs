fn main() {
    println!("{}", greeting("DeepCode"));
}

fn greeting(name: &str) -> String {
    format!("Hello, {name}")
}
