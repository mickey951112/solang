use resolver::{Contract, Namespace};
use Target;

pub mod ethereum;
pub mod substrate;

pub fn generate_abi(contract: &Contract, ns: &Namespace, verbose: bool) -> (String, &'static str) {
    match ns.target {
        Target::Ewasm | Target::Sabre => {
            if verbose {
                eprintln!(
                    "info: Generating Ethereum ABI for contract {}",
                    contract.name
                );
            }

            let abi = ethereum::gen_abi(contract, ns);

            (serde_json::to_string(&abi).unwrap(), "abi")
        }
        Target::Substrate => {
            if verbose {
                eprintln!(
                    "info: Generating Substrate ABI for contract {}",
                    contract.name
                );
            }

            let abi = substrate::gen_abi(contract);

            (serde_json::to_string_pretty(&abi).unwrap(), "json")
        }
    }
}
