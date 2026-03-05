fn main() {
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/mentci_user.capnp")
        .run()
        .expect("schema compiler command");
}
