fn main() {
    println!("Hello, Gloss!");
}

#[cfg(test)]
mod tests {
    #[test]
    fn hello_world() {
        assert_eq!(2 + 2, 4);
    }
}
