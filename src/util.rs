/// 領域展開
/// Finds indices of items matching the `Coll` predicate in both directions from `center`.
/// Continues search while `Cont` predicate holds true.
/// The returned `Vec` also contains `center`
pub fn domain_expansion<T, Cont, Coll>(
    slice: &[T],
    center: usize,
    mut p_continue: Cont,
    mut p_collect: Coll,
) -> Vec<usize>
where
    Cont: FnMut(&T) -> bool,
    Coll: FnMut(&T) -> bool,
{
    let mut indices = vec![center];
    let mut cursor = center;
    loop {
        if cursor == 0 {
            break;
        }
        cursor -= 1;
        if !p_continue(&slice[cursor]) {
            break;
        }
        if p_collect(&slice[cursor]) {
            indices.push(cursor);
        }
    }
    cursor = center;
    loop {
        if cursor >= slice.len() {
            break;
        }
        cursor += 1;
        if !p_continue(&slice[cursor]) {
            break;
        }
        if p_collect(&slice[cursor]) {
            indices.push(cursor);
        }
    }
    indices
}
