use borsh::BorshSerialize;
use sha2::{Digest, Sha256};
use solana_sdk::pubkey::Pubkey;

#[derive(BorshSerialize)]
struct HotVerifyDepositArgs {
    msg_hash: Vec<u8>,
    sender: Pubkey,
    receiver: [u8; 32],
    mint: Pubkey,
    amount: u64,
    nonce: u128,
}

// Anchor discriminator = first 8 bytes of sha256("global:<name>")
fn anchor_discriminator(ix_name: &str) -> [u8; 8] {
    let mut hasher = Sha256::new();
    hasher.update(format!("global:{}", ix_name).as_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest[..8]);
    out
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use anyhow::{anyhow, Context};
    use borsh::BorshSerialize;
    use solana_client::nonblocking::rpc_client::RpcClient;
    use solana_client::rpc_config::RpcSimulateTransactionConfig;
    use solana_commitment_config::CommitmentConfig;
    use solana_sdk::instruction::{AccountMeta, Instruction};
    use solana_sdk::message::Message;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::transaction::Transaction;
    use crate::solana::{anchor_discriminator, HotVerifyDepositArgs};

    async fn must_exist(client: &RpcClient, label: &str, key: &Pubkey) -> anyhow::Result<()> {
        match client.get_account(key).await {
            Ok(acc) => {
                println!(
                    "[ok] {label} {} | lamports={} owner={} exec={}",
                    key,
                    acc.lamports,
                    acc.owner,
                    acc.executable
                );
                Ok(())
            }
            Err(e) => Err(anyhow!("{} {} not found: {}", label, key, e)),
        }
    }

    #[tokio::test]
    async fn foo() -> anyhow::Result<()> {
        let proof_hash_hex = "47b8b751a0d90d113e4e16678ebda646a01a02d376f49f666ddd17ee9f383c2f";
        let sender_str      = "5eMysQ7ywu4D8pmN5RtDoPxbu5YbiEThQy8gaBcmMoho";
        let receiver_b58    = "BJu6S7gT4gnx7AXPnghM7aYiS5dPfSUixqAZJq1Uqf4V"; // IDL wants [u8;32]
        let mint_str        = "BYPsjxa3YuZESQz1dKuBw1QSFCSpecsm8nCQhY5xbU1Z";
        let amount: u64     = 10_000_000;
        let nonce_str       = "1757984522000007228";

        // Accounts (in IDL order for hotVerifyDeposit):
        // signer (mut, signer), deposit (readonly), user (readonly), state (mut)
        let deposit_str     = "GRmeLkQAVHDFBPrSBZ7jBhCwMhEBrMdCFzLKfxhxnUcx";
        let user_str        = "uSCWARfV7dxmvv9kUfBjuHCC5UjXgDRMxgKmhop6vQf";
        let state_str       = "hCofXYTiYHwCPpgVpLvd3VgpapmhqAeNU26bWZANmS8";
        let program_id_str  = "8sXzdKW2jFj7V5heRwPMcygzNH3JZnmie5ZRuNoTuKQC";

        // ---------- RPC ----------
        let rpc_url = std::env::var("RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
        let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

        // ---------- Use an existing on-chain pubkey as the "signer" (no actual signing) ----------
        // Using the provided `sender` address so the account loader can find it.
        let signer_pubkey = Pubkey::from_str(sender_str)
            .with_context(|| "Invalid sender pubkey")?;

        // ---------- Parse & prepare args ----------
        let msg_hash = hex::decode(proof_hash_hex)
            .with_context(|| "Invalid proof_hash hex")?;

        let sender = signer_pubkey; // same as above, for clarity

        // receiver is [u8;32] in IDL. The input is a base58 string; decode to 32 bytes.
        let receiver_bytes = bs58::decode(receiver_b58)
            .into_vec()
            .with_context(|| "receiver base58 decode failed")?;
        if receiver_bytes.len() != 32 {
            return Err(anyhow!(
                "receiver decoded length = {}, expected 32",
                receiver_bytes.len()
            ));
        }
        let mut receiver = [0u8; 32];
        receiver.copy_from_slice(&receiver_bytes);

        let mint = Pubkey::from_str(mint_str)
            .with_context(|| "Invalid mint pubkey")?;

        let nonce: u128 = nonce_str.parse()
            .with_context(|| "Invalid nonce (u128 parse)")?;

        let args = HotVerifyDepositArgs {
            msg_hash,
            sender,
            receiver,
            mint,
            amount,
            nonce,
        };

        // ---------- Instruction data (discriminator + borsh(args)) ----------
        let mut data = Vec::with_capacity(8 + 128);
        data.extend_from_slice(&anchor_discriminator("hotVerifyDeposit"));
        args.serialize(&mut data)?;

        // ---------- Accounts ----------
        let deposit = Pubkey::from_str(deposit_str)?;
        let user    = Pubkey::from_str(user_str)?;
        let state   = Pubkey::from_str(state_str)?;
        let program_id = Pubkey::from_str(program_id_str)?;

        // Preflight: ensure everything exists (and program is executable)
        must_exist(&client, "PROGRAM", &program_id).await?;
        must_exist(&client, "DEPOSIT", &deposit).await?;
        must_exist(&client, "USER   ", &user).await?;
        must_exist(&client, "STATE  ", &state).await?;
        must_exist(&client, "SIGNER ", &signer_pubkey).await?;

        // Per IDL: signer[mut, signer], deposit[readonly], user[readonly], state[mut]
        let accounts = vec![
            AccountMeta::new(signer_pubkey, true),       // signer (marked as signer; no real signature needed)
            AccountMeta::new_readonly(deposit, false),   // deposit
            AccountMeta::new_readonly(user, false),      // user
            AccountMeta::new(state, false),              // state (writable)
        ];

        let ix = Instruction {
            program_id,
            accounts,
            data,
        };

        // ---------- Build an *unsigned* tx just for simulation ----------
        // We do NOT fetch a blockhash or sign; RPC will replace blockhash during sim.
        let msg = Message::new(&[ix], Some(&signer_pubkey));
        let tx = Transaction::new_unsigned(msg);

        // ---------- Simulate (no sig verification; replace recent blockhash) ----------
        let sim_cfg = RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: true,
            commitment: Some(CommitmentConfig::processed()),
            ..RpcSimulateTransactionConfig::default()
        };

        let sim_res = client.simulate_transaction_with_config(&tx, sim_cfg).await?;

        // ---------- Output ----------
        if let Some(logs) = sim_res.value.logs {
            println!("--- Simulation logs ---");
            for l in logs {
                println!("{}", l);
            }
        }
        if let Some(units) = sim_res.value.units_consumed {
            println!("Compute units consumed: {}", units);
        }
        if let Some(err) = sim_res.value.err {
            println!("❌ Simulation error: {:?}", err);
        } else {
            println!("✅ Simulation succeeded.");
        }

        Ok(())
    }
}
