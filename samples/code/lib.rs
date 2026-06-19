//! Sample Rust source — opens in the highlighted text viewer.
pub struct Tile { pub w: u32, pub h: u32 }

impl Tile {
    pub fn area(&self) -> u32 { self.w * self.h }
}

fn main() {
    let t = Tile { w: 4, h: 3 };
    println!("area = {}", t.area());
}
