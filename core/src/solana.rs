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
    use primitive_types::U128;
    use solana_client::nonblocking::rpc_client::RpcClient;
    use solana_client::rpc_config::RpcSimulateTransactionConfig;
    use solana_commitment_config::CommitmentConfig;
    use solana_sdk::instruction::{AccountMeta, Instruction};
    use solana_sdk::message::{Address, Message};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::transaction::Transaction;
    use super::{anchor_discriminator, HotVerifyDepositArgs};

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
        let program_id = Pubkey::from_str("8sXzdKW2jFj7V5heRwPMcygzNH3JZnmie5ZRuNoTuKQC")?;

        let proof_hash_hex = "47b8b751a0d90d113e4e16678ebda646a01a02d376f49f666ddd17ee9f383c2f";
        let sender_str      = "5eMysQ7ywu4D8pmN5RtDoPxbu5YbiEThQy8gaBcmMoho";
        let receiver_b58    = "BJu6S7gT4gnx7AXPnghM7aYiS5dPfSUixqAZJq1Uqf4V";
        let mint_str        = "BYPsjxa3YuZESQz1dKuBw1QSFCSpecsm8nCQhY5xbU1Z";
        let amount: u64     = 10_000_000;
        let nonce_str       = "1757984522000007228";

        // ----------

        let user = {
            let decoded_sender = bs58::decode(sender_str).into_vec()?;
            let seed: &[&[u8]] = &[
                b"user",
                &decoded_sender,
            ];
            let (pda, _bump) = Pubkey::find_program_address(seed, &program_id);
            let expected_user_str = "uSCWARfV7dxmvv9kUfBjuHCC5UjXgDRMxgKmhop6vQf";
            assert_eq!(
                pda.to_string(),
                expected_user_str,
                "program derived address mismatch"
            );
            pda
        };

        let deposit = {
            let sender = bs58::decode(sender_str).into_vec()?;
            let nonce = U128::from_dec_str(nonce_str).unwrap().to_big_endian();
            let receiver = bs58::decode(receiver_b58).into_vec()?;
            let mint = bs58::decode(mint_str).into_vec()?;
            let amount: [u8; 8] = amount.to_be_bytes();
            let seed: &[&[u8]] = &[
                b"deposit",
                &nonce,
                &sender,
                &receiver,
                &mint,
                &amount,
            ];
            let (pda, _bump) = Pubkey::find_program_address(seed, &program_id);
            let expected_deposit_str     = "GRmeLkQAVHDFBPrSBZ7jBhCwMhEBrMdCFzLKfxhxnUcx";
            assert_eq!(
                pda.to_string(),
                expected_deposit_str,
                "program derived address mismatch"
            );
            pda
        };

        let state  = {
            let (pda, _bump) = Pubkey::find_program_address(&[b"state"], &program_id);
            let expected_state_str = "hCofXYTiYHwCPpgVpLvd3VgpapmhqAeNU26bWZANmS8";
            assert_eq!(
                pda.to_string(),
                expected_state_str,
                "program derived address mismatch"
            );
            pda
        };

        // ---------- RPC ----------
        let rpc_url = std::env::var("RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
        let client = RpcClient::new(rpc_url);

        // ---------- Use an existing on-chain pubkey as the "signer" (no actual signing) ----------
        let signer_pubkey = Pubkey::from_str(sender_str)
            .with_context(|| "Invalid sender pubkey")?;

        // ---------- Parse & prepare args ----------
        let msg_hash = hex::decode(proof_hash_hex)
            .with_context(|| "Invalid proof_hash hex")?;

        let sender = signer_pubkey; // same as provided sender

        // receiver is [u8;32] → decode base58 into 32 bytes
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

        // ---------- Build instruction data (snake_case discriminator) ----------
        let mut data = Vec::with_capacity(8 + 128);
        data.extend_from_slice(&anchor_discriminator("hot_verify_deposit"));
        args.serialize(&mut data)?;

        // Preflight: ensure everything exists
        must_exist(&client, "PROGRAM", &program_id).await?;
        must_exist(&client, "DEPOSIT", &deposit).await?;
        must_exist(&client, "USER   ", &user).await?;
        must_exist(&client, "STATE  ", &state).await?;
        must_exist(&client, "SIGNER ", &signer_pubkey).await?;

        // Per IDL: signer[mut, signer], deposit[readonly], user[readonly], state[mut]
        let accounts = vec![
            AccountMeta::new(signer_pubkey, true),
            AccountMeta::new_readonly(deposit, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new(state, false),
        ];

        let ix = Instruction { program_id, accounts, data };

        // ---------- Unsigned tx for simulation ----------
        let msg = Message::new(&[ix], Some(&signer_pubkey));
        let tx = Transaction::new_unsigned(msg);

        // ---------- Simulate ----------
        let sim_cfg = RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: true,
            commitment: Some(CommitmentConfig::confirmed()),
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
