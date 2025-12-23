fn main() {
    capnpc::CompilerCommand::new()
        .file("schemas/trade.capnp")
        .run()
        .expect("compiling schema");
}
