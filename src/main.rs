fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    smol::block_on(ow_my_lib::run())
}
