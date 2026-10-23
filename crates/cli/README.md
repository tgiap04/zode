# Cli

## Testing

You can test your changes to the `cli` crate by first building the main `zode` binary:

```
cargo build -p zode
```

And then building and running the `cli` crate with the following parameters:

```
 cargo run -p cli -- --zode ./target/debug/zode.exe
```
