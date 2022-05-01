use anyhow::bail;
use blockchain_core::SecretAddress;
use clap::Parser;

#[derive(Debug, Parser)]
struct BcAddrArgs {
    /// Enable when creatig new address
    #[clap(short, long)]
    create: bool,

    /// File path to secret address
    #[clap(short, long)]
    address: Option<String>,

    /// File path to secret address
    #[clap(short, long)]
    output: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = BcAddrArgs::parse();

    if args.create {
        let output = match &args.output {
            Some(o) => o,
            None => bail!("Provide output destination."),
        };

        let address = SecretAddress::create();
        bcaddr::write_address(output, &address)?;
    } else {
        let input = match &args.address {
            Some(i) => i,
            None => bail!("Provide address file."),
        };
        let address = bcaddr::read_address(input).map(|addr| addr.to_public_address())?;
        println!("Public address: {}", address);
    }

    Ok(())
}
