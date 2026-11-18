//! Creator and licence record of a stock image, persisted on the node.
//!
//! The record is `jian_ops_schema::node::ImageCredit`, an optional field of
//! the shared node base (`imageCredit` in the `.op` file). It lives on the node
//! that OWNS the image: an `image` node, or the frame / rectangle whose image
//! fill carries the picture. Because it is part of the node it follows every
//! node operation (save, load, copy, paste, undo) with no side table.
//!
//! The rule that keeps it honest: whenever an image is replaced by something
//! that is not a provider result, the credit is cleared in the same mutation.
//! [`node_image_source`] is the comparison key for "did the picture change".

use crate::fills::node_fills;
use crate::PenNodeExt as _;
use jian_ops_schema::node::PenNode;
use jian_ops_schema::style::PenFill;

pub use jian_ops_schema::node::ImageCredit;

/// The credit stored on `node`, if any.
pub fn node_image_credit(node: &PenNode) -> Option<&ImageCredit> {
    node.base().image_credit.as_ref()
}

/// Replace (`Some`) or clear (`None`) the credit stored on `node`.
pub fn set_node_image_credit(node: &mut PenNode, credit: Option<ImageCredit>) {
    node.base_mut().image_credit = credit;
}

/// Every picture url the node paints itself: the `src` of an image node and
/// the url of each image fill. Two equal values mean the picture is unchanged.
pub fn node_image_source(node: &PenNode) -> Vec<&str> {
    let mut sources = Vec::new();
    if let PenNode::Image(image) = node {
        sources.push(image.src.as_str());
    }
    for fill in node_fills(node).into_iter().flatten() {
        if let PenFill::Image(body) = fill {
            sources.push(body.url.as_str());
        }
    }
    sources
}

#[cfg(test)]
#[path = "image_credit_tests.rs"]
mod tests;
