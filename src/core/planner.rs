use crate::core::model::{Fragment, FragmentKey, FragmentState};

pub fn plan_ranges(total: u64, chunk_size: u64) -> Vec<Fragment> {
    if total == 0 {
        return vec![Fragment {
            key: FragmentKey::Range { offset: 0, len: 0 },
            state: FragmentState::Missing,
            retry: 0,
        }];
    }

    let mut frags = Vec::new();
    let mut offset = 0u64;
    while offset < total {
        let remaining = total - offset;
        let len = remaining.min(chunk_size);
        frags.push(Fragment {
            key: FragmentKey::Range { offset, len },
            state: FragmentState::Missing,
            retry: 0,
        });
        offset += len;
    }
    frags
}
