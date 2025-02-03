mod error;
pub mod fs;
pub mod parser;
pub mod repl;
pub mod repl_v2;
mod complete_command;
pub mod system;

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
