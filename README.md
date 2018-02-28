# threadpool
A simple thread pool.

## Example

```rust
extern crate threadpool;

use std::sync::Arc;

use threadpool::Task;
use threadpool::Builder;

struct Simple {
    name: String,
}

impl Task for Simple {
    fn run(&mut self) {
        println!("{} done", self.name);
    }
}

fn main() {
    let pool = Builder::new()
        .worker_count(4)
        .name("simple_thread_pool")
        .build();
    for i in 0..100 {
        let s = Simple {
            name: format!("{}", i),
        };
        pool.spawn(s);
    }
}
```
