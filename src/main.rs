



fn main() {
    tracing_subscriber::fmt::init();
    smol::block_on(rpa_gpu::run())
}

