fn main() {
    prost_build::Config::new()
        .include_file("proto.rs")
        .compile_protos(&["./proto/push.proto"], &["./proto"])
        .expect("failed to compile protos");
}
