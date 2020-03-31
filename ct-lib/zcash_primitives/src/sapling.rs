//! Structs and constants specific to the Sapling shielded pool.
use std::hash::{Hash, Hasher};
use heapsize::HeapSizeOf;

use ff::{BitIterator, PrimeField, PrimeFieldRepr};
use pairing::bls12_381::{Bls12, Fr, FrRepr};
use rand::OsRng;
use sapling_crypto::{
    jubjub::{fs::Fs, FixedGenerators, JubjubBls12},
    pedersen_hash::{pedersen_hash, Personalization},
    primitives::Note,
    redjubjub::{PrivateKey, PublicKey, Signature},
};
use std::io::{self, Read, Write};

use crate::merkle_tree::Hashable;
use JUBJUB;

pub const SAPLING_COMMITMENT_TREE_DEPTH: usize =
    sapling_crypto::circuit::sapling::TREE_DEPTH;

/// Compute a parent node in the Sapling commitment tree given its two children.
pub fn merkle_hash(depth: usize, lhs: &FrRepr, rhs: &FrRepr) -> FrRepr {
    let lhs = {
        let mut tmp = [false; 256];
        for (a, b) in tmp.iter_mut().rev().zip(BitIterator::new(lhs)) {
            *a = b;
        }
        tmp
    };

    let rhs = {
        let mut tmp = [false; 256];
        for (a, b) in tmp.iter_mut().rev().zip(BitIterator::new(rhs)) {
            *a = b;
        }
        tmp
    };

    pedersen_hash::<Bls12, _>(
        Personalization::MerkleTree(depth),
        lhs.iter()
            .map(|&x| x)
            .take(Fr::NUM_BITS as usize)
            .chain(rhs.iter().map(|&x| x).take(Fr::NUM_BITS as usize)),
        &JUBJUB,
    )
    .into_xy()
    .0
    .into_repr()
}

/// A node within the Sapling commitment tree.
#[derive(Clone, Copy, Eq, Debug, PartialEq)]
pub struct Node {
    repr: FrRepr,
}

impl Node {
    pub fn new(repr: FrRepr) -> Self {
        Node { repr }
    }
}

impl HeapSizeOf for Node {
	fn heap_size_of_children(&self) -> usize {
		256
	}
}

impl Hash for Node {
	fn hash<H: Hasher>(&self, state: &mut H) {
		state.write_u64(self.repr.0[0]);
		state.write_u64(self.repr.0[1]);
		state.write_u64(self.repr.0[2]);
		state.write_u64(self.repr.0[3]);
		state.finish();
	}
}

impl Hashable for Node {
    fn read<R: Read>(mut reader: R) -> io::Result<Self> {
        let mut repr = FrRepr::default();
        repr.read_le(&mut reader)?;
        Ok(Node::new(repr))
    }

    fn write<W: Write>(&self, mut writer: W) -> io::Result<()> {
        self.repr.write_le(&mut writer)
    }

    fn combine(depth: usize, lhs: &Self, rhs: &Self) -> Self {
        Node {
            repr: merkle_hash(depth, &lhs.repr, &rhs.repr),
        }
    }

    fn blank() -> Self {
        Node {
            repr: Note::<Bls12>::uncommitted().into_repr(),
        }
    }

    fn empty_root(depth: usize) -> Self {
        EMPTY_ROOTS[depth]
    }
}

impl From<Node> for Fr {
    fn from(node: Node) -> Self {
        Fr::from_repr(node.repr).expect("Tree nodes should be in the prime field")
    }
}

lazy_static! {
    static ref EMPTY_ROOTS: Vec<Node> = {
        let mut v = vec![Node::blank()];
        for d in 0..SAPLING_COMMITMENT_TREE_DEPTH {
            let next = Node::combine(d, &v[d], &v[d]);
            v.push(next);
        }
        v
    };
}

/// Create the spendAuthSig for a Sapling SpendDescription.
pub fn spend_sig(
    ask: PrivateKey<Bls12>,
    ar: Fs,
    sighash: &[u8; 32],
    params: &JubjubBls12,
) -> Signature {
    // Initialize secure RNG
    let mut rng = OsRng::new().expect("should be able to construct RNG");

    // We compute `rsk`...
    let rsk = ask.randomize(ar);

    // We compute `rk` from there (needed for key prefixing)
    let rk = PublicKey::from_private(&rsk, FixedGenerators::SpendingKeyGenerator, params);

    // Compute the signature's message for rk/spend_auth_sig
    let mut data_to_be_signed = [0u8; 64];
    rk.0.write(&mut data_to_be_signed[0..32])
        .expect("message buffer should be 32 bytes");
    (&mut data_to_be_signed[32..64]).copy_from_slice(&sighash[..]);

    // Do the signing
    rsk.sign(
        &data_to_be_signed,
        &mut rng,
        FixedGenerators::SpendingKeyGenerator,
        params,
    )
}
