use crate::Base58;
use crate::Base58Array;
use anyhow::Result;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use serde_with::{hex::Hex, serde_as};
use solana_message::{AccountMeta, Address, Instruction, Message};
use solana_pubkey::Pubkey;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub enum SolanaInputData {
    Deposit(DepositData),
    CheckCompletedWithdrawal(CompletedWithdrawalData),
}

#[serde_as]
#[derive(
    Debug, Serialize, Deserialize, BorshSerialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone,
)]
pub struct CompletedWithdrawalData {
    #[serde(with = "crate::integer::u128_string")]
    #[schemars(with = "String")]
    pub nonce: u128,
    #[serde_as(as = "Base58Array<32>")]
    #[schemars(with = "String")]
    pub receiver: [u8; 32],
}

#[serde_as]
#[derive(
    Debug, Serialize, Deserialize, BorshSerialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone,
)]
pub struct DepositData {
    #[serde_as(as = "Hex")]
    #[schemars(with = "String")]
    pub proof: Vec<u8>,
    #[serde_as(as = "Base58Array<32>")]
    #[schemars(with = "String")]
    pub sender: [u8; 32],
    #[serde_as(as = "Base58Array<32>")]
    #[schemars(with = "String")]
    pub receiver: [u8; 32],
    #[serde_as(as = "Base58")]
    #[schemars(with = "String")]
    pub mint: Vec<u8>,
    pub amount: u64,
    #[serde(with = "crate::integer::u128_string")]
    #[schemars(with = "String")]
    pub nonce: u128,
}

#[derive(Debug, BorshDeserialize)]
pub struct UserAccount {
    _version: u8,
    _bump: u8,
    _last_deposit_nonce: u128,
    pub last_withdraw_nonce: u128,
}

pub mod anchor {
    use sha2::{Digest, Sha256};

    #[derive(Debug, Clone, Copy)]
    enum AnchorDiscKind {
        Account,
        Global, // instruction
    }

    impl AnchorDiscKind {
        #[inline]
        fn prefix(self) -> &'static str {
            match self {
                AnchorDiscKind::Account => "account",
                AnchorDiscKind::Global => "global",
            }
        }
    }

    /// Compute Anchor 8-byte discriminator for accounts/instructions.
    /// - Account:     sha256("account:<Name>")[..8]
    /// - Instruction: sha256("global:<`rust_fn_name`>")[..8]
    fn anchor_discriminator(kind: AnchorDiscKind, name: &str) -> [u8; 8] {
        let mut h = Sha256::new();
        h.update(kind.prefix().as_bytes());
        h.update(b":");
        h.update(name.as_bytes());
        let digest = h.finalize();

        let mut out = [0u8; 8];
        out.copy_from_slice(&digest[..8]);
        out
    }

    #[inline]
    #[must_use]
    pub fn account_discriminator(name: &str) -> [u8; 8] {
        anchor_discriminator(AnchorDiscKind::Account, name)
    }

    #[inline]
    #[must_use]
    pub fn instruction_discriminator(rust_fn_name: &str) -> [u8; 8] {
        // Use the Rust handler name (snake_case), e.g., "hot_verify_deposit"
        anchor_discriminator(AnchorDiscKind::Global, rust_fn_name)
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn user_discriminator() {
            assert_eq!(
                super::anchor_discriminator(super::AnchorDiscKind::Account, "User"),
                [159, 117, 95, 227, 239, 151, 58, 236,]
            );
        }
    }
}

impl CompletedWithdrawalData {
    #[must_use]
    pub fn get_user_address(&self, program_id: &Address) -> Address {
        let seed: &[&[u8]] = &[b"user", &self.receiver];
        let (pda, _bump) = Pubkey::find_program_address(seed, program_id);
        pda
    }
}

impl DepositData {
    fn get_user_address(&self, program_id: &Address) -> Address {
        let seed: &[&[u8]] = &[b"user", &self.sender];
        let (pda, _bump) = Pubkey::find_program_address(seed, program_id);
        pda
    }

    fn get_deposit_address(&self, program_id: &Address) -> Address {
        let seed: &[&[u8]] = &[
            b"deposit",
            &self.nonce.to_be_bytes(),
            &self.sender,
            &self.receiver,
            &self.mint,
            &self.amount.to_be_bytes(),
        ];
        let (pda, _bump) = Pubkey::find_program_address(seed, program_id);
        pda
    }

    fn get_state_address(program_id: &Address) -> Address {
        let (pda, _bump) = Pubkey::find_program_address(&[b"state"], program_id);
        pda
    }

    fn get_instruction(&self, program_id: &Address, method_name: &str) -> Result<Instruction> {
        let sender = Pubkey::from(self.sender);
        let deposit = self.get_deposit_address(program_id);
        let user = self.get_user_address(program_id);
        let state = Self::get_state_address(program_id);

        let accounts = vec![
            AccountMeta::new(sender, true),
            AccountMeta::new_readonly(deposit, false),
            AccountMeta::new_readonly(user, false),
            AccountMeta::new(state, false),
        ];

        let mut data = Vec::with_capacity(8 + 128 /* rough estimation */);
        data.extend_from_slice(&anchor::instruction_discriminator(method_name));
        BorshSerialize::serialize(&self, &mut data)?;

        Ok(Instruction {
            program_id: *program_id,
            accounts,
            data,
        })
    }

    pub fn get_message(&self, program_id: &Address, method_name: &str) -> Result<Message> {
        // We dont care about specific signer here, since there's no signature checking.
        // But we need to provide an existent signer.
        let signer_pubkey = Pubkey::from(self.sender);
        let ix = self.get_instruction(program_id, method_name)?;
        let msg = Message::new(&[ix], Some(&signer_pubkey));
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use crate::bridge::solana::DepositData;
    use anyhow::Result;
    use serde_json::json;
    use solana_pubkey::Pubkey;
    use std::str::FromStr;

    fn get_deposit_data() -> DepositData {
        let json = json!({
            "proof": "47b8b751a0d90d113e4e16678ebda646a01a02d376f49f666ddd17ee9f383c2f",
            "sender": "5eMysQ7ywu4D8pmN5RtDoPxbu5YbiEThQy8gaBcmMoho",
            "receiver": "BJu6S7gT4gnx7AXPnghM7aYiS5dPfSUixqAZJq1Uqf4V",
            "mint": "BYPsjxa3YuZESQz1dKuBw1QSFCSpecsm8nCQhY5xbU1Z",
            "amount": 10_000_000,
            "nonce": "1757984522000007228"
        });
        serde_json::from_value::<DepositData>(json).unwrap()
    }

    #[test]
    fn deserialize_deposit_data() {
        get_deposit_data();
    }

    #[test]
    fn get_user_address() -> Result<()> {
        let program_id = Pubkey::from_str("8sXzdKW2jFj7V5heRwPMcygzNH3JZnmie5ZRuNoTuKQC")?;
        let deposit = get_deposit_data();
        let actual = deposit.get_user_address(&program_id);
        let expected = "uSCWARfV7dxmvv9kUfBjuHCC5UjXgDRMxgKmhop6vQf";
        assert_eq!(actual.to_string(), expected);
        Ok(())
    }

    #[test]
    fn get_state_address() -> Result<()> {
        let program_id = Pubkey::from_str("8sXzdKW2jFj7V5heRwPMcygzNH3JZnmie5ZRuNoTuKQC")?;
        let actual = DepositData::get_state_address(&program_id);
        let expected = "hCofXYTiYHwCPpgVpLvd3VgpapmhqAeNU26bWZANmS8";
        assert_eq!(actual.to_string(), expected);
        Ok(())
    }

    #[test]
    fn get_deposit_address() -> Result<()> {
        let program_id = Pubkey::from_str("8sXzdKW2jFj7V5heRwPMcygzNH3JZnmie5ZRuNoTuKQC")?;
        let deposit = get_deposit_data();
        let actual = deposit.get_deposit_address(&program_id);
        let expected = "GRmeLkQAVHDFBPrSBZ7jBhCwMhEBrMdCFzLKfxhxnUcx";
        assert_eq!(actual.to_string(), expected);
        Ok(())
    }
}
