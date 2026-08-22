/*
 * Copyright (c) 2015 Apple Inc. All rights reserved.
 *
 * @APPLE_LICENSE_HEADER_START@
 *
 * This file contains Original Code and/or Modifications of Original Code
 * as defined in and that are subject to the Apple Public Source License
 * Version 2.0 (the 'License'). You may not use this file except in
 * compliance with the License. Please obtain a copy of the License at
 * http://www.opensource.apple.com/apsl/ and read it before using this
 * file.
 *
 * The Original Code and all software distributed under the License are
 * distributed on an 'AS IS' basis, WITHOUT WARRANTY OF ANY KIND, EITHER
 * EXPRESS OR IMPLIED, AND APPLE HEREBY DISCLAIMS ALL SUCH WARRANTIES,
 * INCLUDING WITHOUT LIMITATION, ANY WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE, QUIET ENJOYMENT OR NON-INFRINGEMENT.
 * Please see the License for the specific language governing rights and
 * limitations under the License.
 *
 * @APPLE_LICENSE_HEADER_END@
 */

/*  CFBinaryPList.c
    Copyright (c) 2000-2014, Apple Inc. All rights reserved.
    Responsibility: Tony Parker
*/

/*
 * Modified 2026-08-22 by plist-rs contributors.
 * The modifications translate allocation-independent immutable/unfiltered
 * graph semantics into executable Rust/Verus and add formal specifications,
 * contracts, invariants, termination measures, and memory-safe indices.
 */

//! Executable graph-semantics layer for the immutable, unfiltered path of
//! `__CFBinaryPlistCreateObjectFiltered` in the pinned CFBinaryPList.c.
//!
//! Bridge boundary: `binary_objects.rs` supplies bounds-checked wire nodes and
//! edges. It must also assign collision-free equality classes for decoded
//! string/data payloads, finite floating values, dates, and recursively
//! normalized containers. This file verifies all graph operations over those
//! finite classes; deriving them from source bytes is outside this unit.

use vstd::prelude::*;

verus! {

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbstractFloat {
    /// All CFNumber NaNs compare equal in the pinned compatibility model.
    pub is_nan: bool,
    /// CFNumber distinguishes negative zero from positive zero and integer 0.
    pub is_negative_zero: bool,
    /// Equal non-NaN, non-sign-mismatched floating values share this class.
    pub value_class: u64,
    /// Present exactly when this float equals an exactly representable integer.
    pub exact_integer: Option<i128>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbstractDate {
    /// Distinct CFDate NaNs compare unequal; identity is handled first.
    pub is_nan: bool,
    /// Equal non-NaN dates, including both zero signs, share this class.
    pub value_class: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberValue {
    Integer(i128),
    Float(AbstractFloat),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeValue {
    Null,
    Bool(bool),
    Number(NumberValue),
    Date(AbstractDate),
    /// Collision-free class of exact bytes.
    Data(u64),
    /// Collision-free class of decoded Unicode scalar/unit content. ASCII and
    /// UTF-16 representations of the same CFString use the same class.
    String(u64),
    /// Value is intentionally not used by equality across distinct identities.
    Uid(u32),
    /// Collision-free deep CFEqual class supplied by the wire-to-graph bridge.
    Array(u64),
    Set(u64),
    Dictionary(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphNode {
    /// Cache and object-identity key used by the pinned reader.
    pub start_offset: u64,
    pub value: NodeValue,
    /// Flat edge span in the separate `edges` vector.
    pub edge_start: usize,
    pub edge_len: usize,
}

pub open spec fn first_offset_spec(
    nodes: Seq<GraphNode>,
    target: u64,
    count: nat,
) -> Option<usize>
    decreases count
{
    if count == 0 || count > nodes.len() {
        None
    } else {
        match first_offset_spec(nodes, target, (count - 1) as nat) {
            Some(first) => Some(first),
            None => if nodes[count - 1].start_offset == target {
                Some((count - 1) as usize)
            } else {
                None
            },
        }
    }
}

pub open spec fn canonical_index_spec(nodes: Seq<GraphNode>, index: usize) -> Option<usize> {
    if index >= nodes.len() {
        None
    } else {
        let target = nodes[index as int].start_offset;
        first_offset_spec(nodes, target, index as nat + 1)
    }
}

/// Extending a searched prefix cannot replace the first offset already found
/// in that prefix.  Keeping this fact separate from the executable search
/// makes the cache specification independent of the loop implementation.
pub proof fn first_offset_stable(
    nodes: Seq<GraphNode>,
    target: u64,
    small: nat,
    large: nat,
    found: usize,
)
    requires
        small <= large,
        large <= nodes.len(),
        first_offset_spec(nodes, target, small) == Some(found),
    ensures
        first_offset_spec(nodes, target, large) == Some(found),
    decreases large - small,
{
    if small < large {
        first_offset_stable(nodes, target, small, (large - 1) as nat, found);
        assert(first_offset_spec(nodes, target, (large - 1) as nat) == Some(found));
        assert(first_offset_spec(nodes, target, large) == Some(found));
    }
}

/// APPLE: the `objects` dictionary lookup keyed by `startOffset`,
/// CFBinaryPList.c:1063-1069 and cache insertions in each immutable branch.
pub fn canonical_index(nodes: &Vec<GraphNode>, index: usize) -> (result: Option<usize>)
    ensures
        result == canonical_index_spec(nodes@, index),
        match result {
            Some(identity) => identity < nodes.len(),
            None => true,
        },
{
    if index >= nodes.len() {
        return None;
    }
    let target = nodes[index].start_offset;
    let mut first = 0usize;
    while first < index
        invariant
            index < nodes.len(),
            first <= index,
            target == nodes@[index as int].start_offset,
            first_offset_spec(nodes@, target, first as nat) == None,
        decreases index - first
    {
        if nodes[first].start_offset == target {
            proof {
                assert(first_offset_spec(nodes@, target, first as nat + 1) == Some(first));
                first_offset_stable(
                    nodes@,
                    target,
                    first as nat + 1,
                    index as nat + 1,
                    first,
                );
                assert(canonical_index_spec(nodes@, index) == Some(first));
            }
            return Some(first);
        }
        first += 1;
    }
    proof {
        assert(first_offset_spec(nodes@, target, index as nat) == None);
        assert(nodes@[index as int].start_offset == target);
        assert(first_offset_spec(nodes@, target, index as nat + 1) == Some(index));
        assert(canonical_index_spec(nodes@, index) == Some(index));
    }
    Some(index)
}

pub open spec fn same_identity_spec(nodes: Seq<GraphNode>, left: usize, right: usize) -> bool {
    match (canonical_index_spec(nodes, left), canonical_index_spec(nodes, right)) {
        (Some(left_id), Some(right_id)) => left_id == right_id,
        _ => false,
    }
}

pub fn same_identity(nodes: &Vec<GraphNode>, left: usize, right: usize) -> (same: bool)
    ensures
        same == same_identity_spec(nodes@, left, right),
{
    match (canonical_index(nodes, left), canonical_index(nodes, right)) {
        (Some(left_id), Some(right_id)) => left_id == right_id,
        _ => false,
    }
}

pub open spec fn primitive_value_spec(value: NodeValue) -> bool {
    match value {
        NodeValue::Array(_) | NodeValue::Set(_) | NodeValue::Dictionary(_) => false,
        _ => true,
    }
}

/// APPLE: `_plistIsPrimitive`, CFBinaryPList.c:849-853.
pub fn primitive_value(value: NodeValue) -> (primitive: bool)
    ensures
        primitive == primitive_value_spec(value),
{
    match value {
        NodeValue::Array(_) | NodeValue::Set(_) | NodeValue::Dictionary(_) => false,
        _ => true,
    }
}

pub open spec fn float_number_equal_spec(left: AbstractFloat, right: AbstractFloat) -> bool {
    if left.is_nan || right.is_nan {
        left.is_nan && right.is_nan
    } else if left.is_negative_zero != right.is_negative_zero {
        false
    } else {
        left.value_class == right.value_class
    }
}

pub fn float_number_equal(left: AbstractFloat, right: AbstractFloat) -> (equal: bool)
    ensures
        equal == float_number_equal_spec(left, right),
{
    if left.is_nan || right.is_nan {
        left.is_nan && right.is_nan
    } else if left.is_negative_zero != right.is_negative_zero {
        false
    } else {
        left.value_class == right.value_class
    }
}

pub open spec fn integer_float_equal_spec(integer: i128, float: AbstractFloat) -> bool {
    !float.is_nan
        && !float.is_negative_zero
        && float.exact_integer == Some(integer)
}

pub fn integer_float_equal(integer: i128, float: AbstractFloat) -> (equal: bool)
    ensures
        equal == integer_float_equal_spec(integer, float),
{
    !float.is_nan
        && !float.is_negative_zero
        && float.exact_integer == Some(integer)
}

pub open spec fn number_equal_spec(left: NumberValue, right: NumberValue) -> bool {
    match (left, right) {
        (NumberValue::Integer(left), NumberValue::Integer(right)) => left == right,
        (NumberValue::Integer(integer), NumberValue::Float(float))
            | (NumberValue::Float(float), NumberValue::Integer(integer)) =>
                integer_float_equal_spec(integer, float),
        (NumberValue::Float(left), NumberValue::Float(right)) =>
            float_number_equal_spec(left, right),
    }
}

pub fn number_equal(left: NumberValue, right: NumberValue) -> (equal: bool)
    ensures
        equal == number_equal_spec(left, right),
{
    match (left, right) {
        (NumberValue::Integer(left), NumberValue::Integer(right)) => left == right,
        (NumberValue::Integer(integer), NumberValue::Float(float))
            | (NumberValue::Float(float), NumberValue::Integer(integer)) =>
                integer_float_equal(integer, float),
        (NumberValue::Float(left), NumberValue::Float(right)) =>
            float_number_equal(left, right),
    }
}

pub open spec fn distinct_value_equal_spec(left: NodeValue, right: NodeValue) -> bool {
    match (left, right) {
        (NodeValue::Null, NodeValue::Null) => true,
        (NodeValue::Bool(left), NodeValue::Bool(right)) => left == right,
        (NodeValue::Number(left), NodeValue::Number(right)) => number_equal_spec(left, right),
        (NodeValue::Date(left), NodeValue::Date(right)) =>
            !left.is_nan && !right.is_nan && left.value_class == right.value_class,
        (NodeValue::Data(left), NodeValue::Data(right)) => left == right,
        (NodeValue::String(left), NodeValue::String(right)) => left == right,
        // The pinned UID runtime class has no equality callback.
        (NodeValue::Uid(_), NodeValue::Uid(_)) => false,
        (NodeValue::Array(left), NodeValue::Array(right)) => left == right,
        (NodeValue::Set(left), NodeValue::Set(right)) => left == right,
        (NodeValue::Dictionary(left), NodeValue::Dictionary(right)) => left == right,
        _ => false,
    }
}

pub fn distinct_value_equal(left: NodeValue, right: NodeValue) -> (equal: bool)
    ensures
        equal == distinct_value_equal_spec(left, right),
{
    match (left, right) {
        (NodeValue::Null, NodeValue::Null) => true,
        (NodeValue::Bool(left), NodeValue::Bool(right)) => left == right,
        (NodeValue::Number(left), NodeValue::Number(right)) => number_equal(left, right),
        (NodeValue::Date(left), NodeValue::Date(right)) =>
            !left.is_nan && !right.is_nan && left.value_class == right.value_class,
        (NodeValue::Data(left), NodeValue::Data(right)) => left == right,
        (NodeValue::String(left), NodeValue::String(right)) => left == right,
        (NodeValue::Uid(_), NodeValue::Uid(_)) => false,
        (NodeValue::Array(left), NodeValue::Array(right)) => left == right,
        (NodeValue::Set(left), NodeValue::Set(right)) => left == right,
        (NodeValue::Dictionary(left), NodeValue::Dictionary(right)) => left == right,
        _ => false,
    }
}

pub open spec fn node_equal_spec(
    nodes: Seq<GraphNode>,
    left: usize,
    right: usize,
) -> bool {
    match (canonical_index_spec(nodes, left), canonical_index_spec(nodes, right)) {
        (Some(left_id), Some(right_id)) => {
            left_id == right_id
                || distinct_value_equal_spec(
                    nodes[left_id as int].value,
                    nodes[right_id as int].value,
                )
        },
        _ => false,
    }
}

/// CFEqual-style equality after cache identity has been applied. Pointer
/// identity wins first, which makes repeated references to the same UID or
/// NaN date equal; distinct UID objects and distinct NaN dates remain unequal.
pub fn node_equal(nodes: &Vec<GraphNode>, left: usize, right: usize) -> (equal: bool)
    ensures
        equal == node_equal_spec(nodes@, left, right),
{
    match (canonical_index(nodes, left), canonical_index(nodes, right)) {
        (Some(left_id), Some(right_id)) => {
            left_id == right_id
                || distinct_value_equal(nodes[left_id].value, nodes[right_id].value)
        },
        _ => false,
    }
}

pub open spec fn active_contains_spec(active: Seq<usize>, identity: usize) -> bool {
    exists|position: int| 0 <= position < active.len()
        && #[trigger] active[position] == identity
}

pub fn active_contains(active: &Vec<usize>, identity: usize) -> (contains: bool)
    ensures
        contains == active_contains_spec(active@, identity),
{
    let mut position = 0usize;
    while position < active.len()
        invariant
            position <= active.len(),
            forall|prior: int| 0 <= prior < position ==>
                #[trigger] active@[prior] != identity,
        decreases active.len() - position
    {
        if active[position] == identity {
            proof {
                assert(active_contains_spec(active@, identity));
            }
            return true;
        }
        position += 1;
    }
    proof {
        assert(!active_contains_spec(active@, identity));
    }
    false
}

pub open spec fn edge_span_valid_spec(edges: Seq<usize>, node: GraphNode) -> bool {
    node.edge_start as int + node.edge_len as int <= usize::MAX as int
        && node.edge_start as int + node.edge_len as int <= edges.len()
}

pub fn edge_span_valid(edges: &Vec<usize>, node: GraphNode) -> (valid: bool)
    ensures
        valid == edge_span_valid_spec(edges@, node),
{
    node.edge_len <= usize::MAX - node.edge_start
        && node.edge_start + node.edge_len <= edges.len()
}

pub open spec fn scalar_value_spec(value: NodeValue) -> bool {
    match value {
        NodeValue::Array(_) | NodeValue::Set(_) | NodeValue::Dictionary(_) => false,
        _ => true,
    }
}

pub open spec fn node_shape_valid_spec(
    nodes: Seq<GraphNode>,
    edges: Seq<usize>,
    index: usize,
) -> bool {
    match canonical_index_spec(nodes, index) {
        None => false,
        Some(identity) => raw_node_shape_valid_spec(nodes, edges, identity),
    }
}

/// Shape check after cache canonicalization.  It is intentionally a separate
/// total specification so traversal never has to canonicalize an identity a
/// second time.
pub open spec fn raw_node_shape_valid_spec(
    nodes: Seq<GraphNode>,
    edges: Seq<usize>,
    identity: usize,
) -> bool {
    if identity >= nodes.len() {
        false
    } else {
        let node = nodes[identity as int];
        edge_span_valid_spec(edges, node)
            && match node.value {
                NodeValue::Dictionary(_) => node.edge_len % 2 == 0,
                NodeValue::Array(_) | NodeValue::Set(_) => true,
                _ => node.edge_len == 0,
            }
    }
}

pub fn raw_node_shape_valid(
    nodes: &Vec<GraphNode>,
    edges: &Vec<usize>,
    identity: usize,
) -> (valid: bool)
    ensures
        valid == raw_node_shape_valid_spec(nodes@, edges@, identity),
{
    if identity >= nodes.len() {
        return false;
    }
    let node = nodes[identity];
    if !edge_span_valid(edges, node) {
        return false;
    }
    match node.value {
        NodeValue::Dictionary(_) => node.edge_len % 2 == 0,
        NodeValue::Array(_) | NodeValue::Set(_) => true,
        _ => node.edge_len == 0,
    }
}

pub fn node_shape_valid(
    nodes: &Vec<GraphNode>,
    edges: &Vec<usize>,
    index: usize,
) -> (valid: bool)
    ensures
        valid == node_shape_valid_spec(nodes@, edges@, index),
{
    let identity = match canonical_index(nodes, index) {
        Some(value) => value,
        None => return false,
    };
    raw_node_shape_valid(nodes, edges, identity)
}

pub open spec fn primitive_node_spec(nodes: Seq<GraphNode>, index: usize) -> bool {
    match canonical_index_spec(nodes, index) {
        Some(identity) => primitive_value_spec(nodes[identity as int].value),
        None => false,
    }
}

pub fn primitive_node(nodes: &Vec<GraphNode>, index: usize) -> (primitive: bool)
    ensures
        primitive == primitive_node_spec(nodes@, index),
{
    match canonical_index(nodes, index) {
        Some(identity) => primitive_value(nodes[identity].value),
        None => false,
    }
}

pub open spec fn edge_window_valid_spec(
    edges: Seq<usize>,
    start: usize,
    count: usize,
) -> bool {
    start as int + count as int <= usize::MAX as int
        && start as int + count as int <= edges.len()
}

pub fn edge_window_valid(
    edges: &Vec<usize>,
    start: usize,
    count: usize,
) -> (valid: bool)
    ensures
        valid == edge_window_valid_spec(edges@, start, count),
{
    count <= usize::MAX - start && start + count <= edges.len()
}

/// Whether the reference at `position` is CFEqual to any earlier reference in
/// the same bounds-checked span.  This is the independent first-occurrence
/// specification used for both CFSet insertion and CFDictionary key insertion.
pub open spec fn earlier_equal_spec(
    nodes: Seq<GraphNode>,
    edges: Seq<usize>,
    start: usize,
    count: usize,
    position: usize,
) -> bool {
    edge_window_valid_spec(edges, start, count)
        && position < count
        && exists|prior: int| 0 <= prior < position
            && node_equal_spec(
                nodes,
                #[trigger] edges[start as int + prior],
                edges[start as int + position as int],
            )
}

pub fn earlier_equal(
    nodes: &Vec<GraphNode>,
    edges: &Vec<usize>,
    start: usize,
    count: usize,
    position: usize,
) -> (duplicate: bool)
    ensures
        duplicate == earlier_equal_spec(nodes@, edges@, start, count, position),
{
    if !edge_window_valid(edges, start, count) || position >= count {
        return false;
    }
    let current = edges[start + position];
    let mut prior = 0usize;
    while prior < position
        invariant
            edge_window_valid_spec(edges@, start, count),
            position < count,
            current == edges@[start as int + position as int],
            prior <= position,
            forall|checked: int| 0 <= checked < prior ==>
                !node_equal_spec(
                    nodes@,
                    #[trigger] edges@[start as int + checked],
                    current,
                ),
        decreases position - prior
    {
        if node_equal(nodes, edges[start + prior], current) {
            proof {
                assert(earlier_equal_spec(nodes@, edges@, start, count, position));
            }
            return true;
        }
        prior += 1;
    }
    proof {
        assert(!earlier_equal_spec(nodes@, edges@, start, count, position));
    }
    false
}

/// APPLE: the normal immutable 0xc0 branch finishes through
/// `__CFSetCreateTransfer` (`CFBinaryPList.c:1416-1424`), whose AddValue
/// construction retains a position exactly when no earlier member is CFEqual.
/// Later duplicates are therefore removed while the first wire occurrence
/// supplies the identity.
pub open spec fn set_position_retained_spec(
    nodes: Seq<GraphNode>,
    edges: Seq<usize>,
    index: usize,
    position: usize,
) -> bool {
    match canonical_index_spec(nodes, index) {
        None => false,
        Some(identity) => {
            if !raw_node_shape_valid_spec(nodes, edges, identity) {
                false
            } else {
                let node = nodes[identity as int];
                match node.value {
                    NodeValue::Set(_) => position < node.edge_len
                        && !earlier_equal_spec(
                            nodes,
                            edges,
                            node.edge_start,
                            node.edge_len,
                            position,
                        ),
                    _ => false,
                }
            }
        },
    }
}

pub fn set_position_retained(
    nodes: &Vec<GraphNode>,
    edges: &Vec<usize>,
    index: usize,
    position: usize,
) -> (retained: bool)
    ensures
        retained == set_position_retained_spec(nodes@, edges@, index, position),
{
    let identity = match canonical_index(nodes, index) {
        Some(value) => value,
        None => return false,
    };
    if !raw_node_shape_valid(nodes, edges, identity) {
        return false;
    }
    let node = nodes[identity];
    match node.value {
        NodeValue::Set(_) => {
            position < node.edge_len
                && !earlier_equal(
                    nodes,
                    edges,
                    node.edge_start,
                    node.edge_len,
                    position,
                )
        },
        _ => false,
    }
}

/// APPLE: the normal immutable dictionary branch reads all keys first, then all
/// values, and finishes through `__CFDictionaryCreateTransfer`
/// (`CFBinaryPList.c:1535-1543`).  Its AddValue construction retains the first
/// equal key and its value, so only that first wire entry is retained by this
/// normalized immutable model.
pub open spec fn dictionary_entry_retained_spec(
    nodes: Seq<GraphNode>,
    edges: Seq<usize>,
    index: usize,
    entry: usize,
) -> bool {
    match canonical_index_spec(nodes, index) {
        None => false,
        Some(identity) => {
            if !raw_node_shape_valid_spec(nodes, edges, identity) {
                false
            } else {
                let node = nodes[identity as int];
                match node.value {
                    NodeValue::Dictionary(_) => {
                        let count = node.edge_len / 2;
                        entry < count
                            && !earlier_equal_spec(
                                nodes,
                                edges,
                                node.edge_start,
                                count,
                                entry,
                            )
                    },
                    _ => false,
                }
            }
        },
    }
}

pub fn dictionary_entry_retained(
    nodes: &Vec<GraphNode>,
    edges: &Vec<usize>,
    index: usize,
    entry: usize,
) -> (retained: bool)
    ensures
        retained == dictionary_entry_retained_spec(nodes@, edges@, index, entry),
{
    let identity = match canonical_index(nodes, index) {
        Some(value) => value,
        None => return false,
    };
    if !raw_node_shape_valid(nodes, edges, identity) {
        return false;
    }
    let node = nodes[identity];
    match node.value {
        NodeValue::Dictionary(_) => {
            let count = node.edge_len / 2;
            entry < count
                && !earlier_equal(
                    nodes,
                    edges,
                    node.edge_start,
                    count,
                    entry,
                )
        },
        _ => false,
    }
}

pub open spec fn child_key_eligible_spec(
    nodes: Seq<GraphNode>,
    edges: Seq<usize>,
    node: GraphNode,
    position: int,
) -> bool
    recommends
        edge_span_valid_spec(edges, node),
        0 <= position < node.edge_len,
{
    let child = edges[node.edge_start as int + position];
    match node.value {
        NodeValue::Dictionary(_) if position < node.edge_len / 2 =>
            primitive_node_spec(nodes, child),
        _ => true,
    }
}

pub open spec fn visit_spec(
    nodes: Seq<GraphNode>,
    edges: Seq<usize>,
    index: usize,
    fuel: nat,
    active: Seq<usize>,
) -> bool
    decreases fuel, 0nat, 0nat
{
    if fuel == 0 {
        false
    } else {
        match canonical_index_spec(nodes, index) {
            None => false,
            Some(identity) => {
                if active_contains_spec(active, identity)
                    || !raw_node_shape_valid_spec(nodes, edges, identity)
                {
                    false
                } else {
                    let node = nodes[identity as int];
                    visit_children_spec(
                        nodes,
                        edges,
                        node,
                        0,
                        (fuel - 1) as nat,
                        active.push(identity),
                    )
                }
            },
        }
    }
}

/// Independent recursive specification for a suffix of a validated edge
/// span.  `visit_spec` lowers fuel before entering it; each child visit is
/// lower in the mutual-recursion ordering, and advancing `position` strictly
/// shrinks the remaining suffix.
pub open spec fn visit_children_spec(
    nodes: Seq<GraphNode>,
    edges: Seq<usize>,
    node: GraphNode,
    position: usize,
    fuel: nat,
    active: Seq<usize>,
) -> bool
    decreases fuel, 1nat, (node.edge_len - position) as nat
{
    if !edge_span_valid_spec(edges, node) || position > node.edge_len {
        false
    } else if position == node.edge_len {
        true
    } else {
        let child = edges[node.edge_start as int + position as int];
        child_key_eligible_spec(nodes, edges, node, position as int)
            && visit_spec(nodes, edges, child, fuel, active)
            && visit_children_spec(
                nodes,
                edges,
                node,
                (position + 1) as usize,
                fuel,
                active,
            )
    }
}

/// Fuel-bounded, active-path traversal. Canonicalization before the active
/// check implements cache-by-start-offset identity; a repeated canonical
/// identity on the current path is rejected as a cycle. Shared completed
/// subgraphs may be revisited, but denote the same identity and acceptance is
/// unchanged in this allocation-independent model.
pub fn visit(
    nodes: &Vec<GraphNode>,
    edges: &Vec<usize>,
    index: usize,
    fuel: usize,
    active: &Vec<usize>,
) -> (valid: bool)
    ensures
        valid == visit_spec(nodes@, edges@, index, fuel as nat, active@),
    decreases fuel, 0nat, 0nat,
{
    if fuel == 0 {
        return false;
    }
    let identity = match canonical_index(nodes, index) {
        Some(value) => value,
        None => return false,
    };
    if active_contains(active, identity) || !raw_node_shape_valid(nodes, edges, identity) {
        return false;
    }

    let node = nodes[identity];
    let mut next_active = active.clone();
    next_active.push(identity);
    visit_children(nodes, edges, node, 0, fuel - 1, &next_active)
}

pub fn visit_children(
    nodes: &Vec<GraphNode>,
    edges: &Vec<usize>,
    node: GraphNode,
    position: usize,
    fuel: usize,
    active: &Vec<usize>,
) -> (valid: bool)
    ensures
        valid == visit_children_spec(
            nodes@,
            edges@,
            node,
            position,
            fuel as nat,
            active@,
        ),
    decreases fuel, 1nat, (node.edge_len - position) as nat,
{
    if !edge_span_valid(edges, node) || position > node.edge_len {
        return false;
    }
    if position == node.edge_len {
        return true;
    }

    let edge_position = node.edge_start + position;
    let child = edges[edge_position];
    let is_dictionary_key = match node.value {
        NodeValue::Dictionary(_) => position < node.edge_len / 2,
        _ => false,
    };
    if is_dictionary_key && !primitive_node(nodes, child) {
        return false;
    }
    if !visit(nodes, edges, child, fuel, active) {
        return false;
    }
    visit_children(
        nodes,
        edges,
        node,
        position + 1,
        fuel,
        active,
    )
}

pub open spec fn graph_valid_spec(
    nodes: Seq<GraphNode>,
    edges: Seq<usize>,
    root: usize,
) -> bool {
    nodes.len() < usize::MAX
        && visit_spec(nodes, edges, root, (nodes.len() + 1) as nat, Seq::empty())
}

pub fn validate_graph(
    nodes: &Vec<GraphNode>,
    edges: &Vec<usize>,
    root: usize,
) -> (valid: bool)
    ensures
        valid == graph_valid_spec(nodes@, edges@, root),
{
    if nodes.len() == usize::MAX {
        return false;
    }
    let active = Vec::<usize>::new();
    visit(nodes, edges, root, nodes.len() + 1, &active)
}

fn main() {
    let nodes = vec![GraphNode {
        start_offset: 8,
        value: NodeValue::Uid(7),
        edge_start: 0,
        edge_len: 0,
    }];
    let _same = node_equal(&nodes, 0, 0);
}

}
