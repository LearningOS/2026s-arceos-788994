#![no_std]
#![no_main]

extern crate alloc;
#[macro_use]
extern crate axstd as std;

// 重点：必须用 hashbrown 而不是 alloc！
use hashbrown::HashMap;

#[no_mangle]
fn main() {
    println!("Running memory tests...");
    test_hashmap();
    println!("Memory tests run OK!");
}

fn test_hashmap() {
    let mut map = HashMap::new();
    map.insert(1, "a");
    map.insert(2, "b");
    map.insert(3, "c");

    for (k, v) in &map {
        println!("{} => {}", k, v);
    }

    println!("test_hashmap() OK!");
}
