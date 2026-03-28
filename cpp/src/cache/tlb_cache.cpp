// ARM SMMU v3 TLB Cache Implementation
// Copyright (c) 2024 John Greninger

#include "smmu/tlb_cache.h"
#include <chrono>
#include <algorithm>
#include <cstdio>
#include <vector>

namespace smmu {

// Constructor
TLBCache::TLBCache(size_t maxSize)
    : maxSize(maxSize > 0 ? maxSize : 1024), effectiveStripes(NUM_LOCK_STRIPES),
      hitCount(0), missCount(0) {
    // For small caches, use fewer effective stripes to ensure each stripe can hold
    // multiple entries.  Minimum 4 entries per active stripe for reasonable LRU behavior.
    // BUG-NEW-B fix: only provision effectiveStripes active stripes so that LRU eviction
    // fires at the correct threshold and getSize() never silently exceeds getCapacity().
    if (this->maxSize < NUM_LOCK_STRIPES * 4) {
        effectiveStripes = (this->maxSize + 3) / 4;  // At least 4 entries per stripe
        if (effectiveStripes < 1) {
            effectiveStripes = 1;
        }
    }

    // Initialize per-stripe max sizes.  Inactive stripes retain maxEntriesPerStripe=0
    // but are never targeted by getLockStripeIndex() (which routes via effectiveStripes),
    // so they will remain empty.
    size_t entriesPerStripe = (this->maxSize + effectiveStripes - 1) / effectiveStripes;
    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        stripes[i].maxEntriesPerStripe = (i < effectiveStripes) ? entriesPerStripe : 0;
    }
}

// Destructor
TLBCache::~TLBCache() {
    clear();
}

// Cache operations - Result<T> error handling pattern
Result<TLBEntry> TLBCache::lookupEntry(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    // Validate input parameters before acquiring lock
    if (streamID > MAX_STREAM_ID) {
        missCount.fetch_add(1, std::memory_order_relaxed);
        return makeError<TLBEntry>(SMMUError::InvalidStreamID);
    }

    if (pasid > MAX_PASID) {
        missCount.fetch_add(1, std::memory_order_relaxed);
        return makeError<TLBEntry>(SMMUError::InvalidPASID);
    }

    CacheKey key = makeKey(streamID, pasid, iova, securityState);
    size_t stripeIndex = getLockStripeIndex(key);
    std::lock_guard<std::mutex> lock(lockStripes[stripeIndex]);

    CacheStripe& stripe = stripes[stripeIndex];
    auto it = stripe.map.find(key);

    if (it == stripe.map.end()) {
        missCount.fetch_add(1, std::memory_order_relaxed);
        return makeError<TLBEntry>(SMMUError::CacheEntryNotFound);
    }

    // Move to front (LRU) within this stripe
    auto listIt = it->second;
    if (listIt != stripe.list.begin()) {
        stripe.list.splice(stripe.list.begin(), stripe.list, listIt);
        stripe.map[key] = stripe.list.begin();
    }

    hitCount.fetch_add(1, std::memory_order_relaxed);

    // Create a copy of the TLBEntry to avoid reference issues
    TLBEntry entryCopy = stripe.list.begin()->second;
    return Result<TLBEntry>(entryCopy);
}

Result<CacheEntry> TLBCache::lookupCacheEntry(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    // Validate input parameters without updating statistics yet
    if (streamID > MAX_STREAM_ID) {
        missCount.fetch_add(1, std::memory_order_relaxed);
        return makeError<CacheEntry>(SMMUError::InvalidStreamID);
    }

    if (pasid > MAX_PASID) {
        missCount.fetch_add(1, std::memory_order_relaxed);
        return makeError<CacheEntry>(SMMUError::InvalidPASID);
    }

    // Use lookupEntry which handles its own statistics - avoid double counting
    Result<TLBEntry> tlbResult = lookupEntry(streamID, pasid, iova, securityState);
    if (tlbResult.isError()) {
        // Don't count miss again - lookupEntry already did
        return makeError<CacheEntry>(tlbResult.getError());
    }

    const TLBEntry& tlbEntry = tlbResult.getValue();
    CacheEntry entry;
    entry.iova = tlbEntry.iova;
    entry.physicalAddress = tlbEntry.physicalAddress;
    entry.permissions = tlbEntry.permissions;
    entry.securityState = tlbEntry.securityState;
    entry.timestamp = tlbEntry.timestamp;

    // Return a copy of the CacheEntry
    return Result<CacheEntry>(std::move(entry));
}

void TLBCache::insert(const TLBEntry& entry) {
    CacheKey key = makeKey(entry.streamID, entry.pasid, entry.iova, entry.securityState);
    size_t stripeIndex = getLockStripeIndex(key);
    std::lock_guard<std::mutex> lock(lockStripes[stripeIndex]);

    CacheStripe& stripe = stripes[stripeIndex];

    // Check if entry already exists
    auto it = stripe.map.find(key);
    if (it != stripe.map.end()) {
        // Update existing entry
        it->second->second = entry;
        // Move to front (LRU)
        auto listIt = it->second;
        if (listIt != stripe.list.begin()) {
            stripe.list.splice(stripe.list.begin(), stripe.list, listIt);
            stripe.map[key] = stripe.list.begin();
        }
        return;
    }

    // Check if stripe is full
    if (stripe.list.size() >= stripe.maxEntriesPerStripe) {
        // Evict LRU entry from this stripe
        if (!stripe.list.empty()) {
            auto last = stripe.list.end();
            --last;
            const CacheKey& evictKey = last->first;

            // Remove from secondary indices - O(1) with unordered_set
            auto& streamSet = stripe.streamIndex[evictKey.streamID];
            streamSet.erase(evictKey);
            if (streamSet.empty()) {
                stripe.streamIndex.erase(evictKey.streamID);
            }

            StreamPASIDKey spKey{evictKey.streamID, evictKey.pasid};
            auto& pasidSet = stripe.pasidIndex[spKey];
            pasidSet.erase(evictKey);
            if (pasidSet.empty()) {
                stripe.pasidIndex.erase(spKey);
            }

            stripe.map.erase(evictKey);
            stripe.list.erase(last);
        }
    }

    // Insert new entry
    stripe.list.push_front(std::make_pair(key, entry));
    stripe.map[key] = stripe.list.begin();

    // Add to secondary indices for O(k) invalidation
    stripe.streamIndex[entry.streamID].insert(key);
    StreamPASIDKey spKey{entry.streamID, entry.pasid};
    stripe.pasidIndex[spKey].insert(key);
}

bool TLBCache::lookup(StreamID streamID, PASID pasid, IOVA iova, CacheEntry& entry,
                      SecurityState securityState) {
    // ARM §3.13: TLB lookups must be keyed on (StreamID, PASID, IOVA, SecurityState).
    // Forward the security state so Secure entries are not missed.
    // BUG-NEW-A fix: use lookupEntry() which copies the TLBEntry before releasing the
    // stripe lock, avoiding a use-after-free via the raw pointer returned by lookup().
    Result<TLBEntry> result = lookupEntry(streamID, pasid, iova, securityState);
    if (result.isOk()) {
        const TLBEntry& tlbEntry = result.getValue();
        entry.iova = tlbEntry.iova;
        entry.physicalAddress = tlbEntry.physicalAddress;
        entry.permissions = tlbEntry.permissions;
        entry.securityState = tlbEntry.securityState;
        entry.timestamp = tlbEntry.timestamp;
        return true;
    }
    return false;
}

void TLBCache::insert(StreamID streamID, PASID pasid, const CacheEntry& entry) {
    TLBEntry tlbEntry;
    tlbEntry.streamID = streamID;
    tlbEntry.pasid = pasid;
    tlbEntry.iova = entry.iova;
    tlbEntry.physicalAddress = entry.physicalAddress;
    tlbEntry.permissions = entry.permissions;
    tlbEntry.securityState = entry.securityState;
    tlbEntry.valid = true;
    tlbEntry.timestamp = entry.timestamp;

    insert(tlbEntry);
}

void TLBCache::remove(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    CacheKey key = makeKey(streamID, pasid, iova, securityState);
    size_t stripeIndex = getLockStripeIndex(key);
    std::lock_guard<std::mutex> lock(lockStripes[stripeIndex]);

    CacheStripe& stripe = stripes[stripeIndex];
    auto it = stripe.map.find(key);

    if (it != stripe.map.end()) {
        // Remove from secondary indices - O(1) with unordered_set
        auto& streamSet = stripe.streamIndex[streamID];
        streamSet.erase(key);
        if (streamSet.empty()) {
            stripe.streamIndex.erase(streamID);
        }

        StreamPASIDKey spKey{streamID, pasid};
        auto& pasidSet = stripe.pasidIndex[spKey];
        pasidSet.erase(key);
        if (pasidSet.empty()) {
            stripe.pasidIndex.erase(spKey);
        }

        stripe.list.erase(it->second);
        stripe.map.erase(it);
    }
}

// Invalidation operations
void TLBCache::invalidate(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) {
    remove(streamID, pasid, iova, securityState);
}

void TLBCache::invalidateByStream(StreamID streamID) {
    invalidateStream(streamID);
}

void TLBCache::invalidateByPASID(StreamID streamID, PASID pasid) {
    invalidatePASID(streamID, pasid);
}

void TLBCache::invalidateAll() {
    clear();
}

void TLBCache::invalidateNonHypEntries() {
    // BUG-QA-14 fix: ARM §4.4.4.1 — CMD_TLBI_NSNH_ALL invalidates only EL1_EL0 entries.
    // Entries tagged with EL2 or EL2_E2H (hypervisor) must be preserved.
    // BUG-NEW-18 fix: ARM §4.4.4.1 — NSNH_ALL is scoped to NonSecure world only.
    // The filter must also check securityState == NonSecure; Secure EL1_EL0
    // entries must be preserved (NSNH = Non-Secure Non-Hyp).
    // Same stripe-iteration pattern as invalidateByASID() / invalidateByVMID().
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->second.strw == StreamWorld::EL1_EL0 &&
                it->second.securityState == SecurityState::NonSecure) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateNonHypEntriesByVMID(uint16_t vmid) {
    // QA-AUDIT-FIX-2: ARM §4.4.2.1 CMD_TLBI_NH_ALL — VMID-scoped Non-Hyp invalidation.
    // Evicts only entries tagged with EL1_EL0 AND whose vmid matches the command operand.
    // Preserves EL2, EL2_E2H, and EL1_EL0 entries belonging to other VMIDs.
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->second.strw == StreamWorld::EL1_EL0 && it->second.vmid == vmid) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateEL2Entries() {
    // BUG-NEW-18 fix: ARM §4.4.2.7 CMD_TLBI_EL2_ALL — evict only EL2 and
    // EL2_E2H tagged TLB entries.  EL1_EL0 entries must be preserved.
    // Mirrors the pattern of invalidateNonHypEntries() which evicts EL1_EL0.
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->second.strw == StreamWorld::EL2 ||
                it->second.strw == StreamWorld::EL2_E2H) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateBySecurityState(SecurityState securityState) {
    // Acquire all locks for bulk invalidation
    AllLocksGuard allLocks(*this);

    // Iterate through all stripes and remove matching entries
    // Note: Security state filtering is less common, so we iterate instead of adding another index
    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->first.securityState == securityState) {
                const CacheKey& key = it->first;

                // Remove from secondary indices - O(1) with unordered_set
                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateByASID(uint16_t asid) {
    // ARM §4.4: CMD_TLBI_NH_ASID / CMD_TLBI_EL2_ASID — evict all TLB entries tagged with this ASID.
    // O(N) scan is acceptable; targeted ASID invalidation is infrequent relative to translations.
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->second.asid == asid) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateByVMID(uint16_t vmid) {
    // ARM §4.4: CMD_TLBI_S12_VMALL / CMD_TLBI_S2_IPA — evict all TLB entries tagged with this VMID.
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->second.vmid == vmid) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateByVMIDWithMask(uint16_t vmid, uint16_t vmidMask) {
    // CONF-GAP-12: VMID wildcard matching — invalidate entries where
    // (entry.vmid & vmidMask) == (vmid & vmidMask).
    // When vmidMask==0xFFFF this is exact matching (same as invalidateByVMID).
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if ((it->second.vmid & vmidMask) == (vmid & vmidMask)) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateByVMIDAndIPA(uint16_t vmid, uint16_t vmidMask, IOVA ipa, IOVA ipaEnd) {
    // CONF-GAP-7: ARM §4.4 TLBI_S2_IPA IPA-selective invalidation.
    // Evict two-stage TLB entries where:
    //   (entry.vmid & vmidMask) == (vmid & vmidMask)  AND
    //   entry.ipa != 0  (two-stage entry — has a valid IPA)  AND
    //   entry.ipa >= ipa AND entry.ipa <= ipaEnd
    // Page-align the IPA operand before comparison (strip sub-page offset bits).
    IOVA pageAlignedIPA    = ipa    & ~static_cast<IOVA>(PAGE_SIZE - 1u);
    IOVA pageAlignedIPAEnd = ipaEnd & ~static_cast<IOVA>(PAGE_SIZE - 1u);
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            const TLBEntry& e = it->second;
            bool vmidMatch = ((e.vmid & vmidMask) == (vmid & vmidMask));
            // CONF-GAP-7: entries with ipa==0 have no IPA info (single-stage or
            // two-stage entry whose IPA was not yet stored). Per spec §4.4, TLBI_S2_IPA
            // must invalidate matching entries; conservatively invalidate on VMID-only
            // when IPA info is unavailable. Entries with ipa!=0 are precisely scoped.
            bool ipaMatch  = (e.ipa == 0u) ||
                             (e.ipa >= pageAlignedIPA &&
                              e.ipa <= pageAlignedIPAEnd);
            if (vmidMatch && ipaMatch) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateByVMIDAndASID(uint16_t vmid, uint16_t asid) {
    // BUG-CPP-2 fix: ARM §4.4.2.2 CMD_TLBI_NH_ASID — evict entries where
    // entry.vmid == vmid AND entry.asid == asid (joint match).
    // Previously the TLBI_NH_ASID case called invalidateByASID() which evicts
    // ALL entries with the given ASID regardless of VMID, over-invalidating
    // entries belonging to other VMIDs that happen to share the same ASID.
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->second.vmid == vmid && it->second.asid == asid) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateByVAAndASID(IOVA va, uint16_t asid) {
    // CONF-GAP-6: VA+ASID targeted TLBI — evict entries where iova matches va AND asid matches.
    // va is page-aligned (strip offset bits) before comparison.
    IOVA pageAlignedVA = va & ~static_cast<IOVA>(PAGE_SIZE - 1u);
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->second.iova == pageAlignedVA && it->second.asid == asid) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateByVA(IOVA va) {
    // CONF-GAP-6: VAA invalidation — evict entries where iova matches va, regardless of ASID.
    IOVA pageAlignedVA = va & ~static_cast<IOVA>(PAGE_SIZE - 1u);
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->second.iova == pageAlignedVA) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateByVARange(IOVA start, IOVA end, uint16_t asid) {
    // CONF-GAP-8: Range-based TLBI — evict entries where iova is in [start, end] AND asid matches.
    IOVA pageAlignedStart = start & ~static_cast<IOVA>(PAGE_SIZE - 1u);
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            IOVA entryVA = it->second.iova;
            if (entryVA >= pageAlignedStart && entryVA <= end && it->second.asid == asid) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateByVMIDAndVAAndASID(uint16_t vmid, IOVA va, uint16_t asid) {
    // BUG-NEW-37 fix: ARM §4.4.2.3 CMD_TLBI_NH_VA — evict entries where
    // entry.vmid == vmid AND entry.iova == va AND entry.asid == asid (joint match).
    // Previously called invalidateByVAAndASID() which ignores VMID, evicting
    // entries from other VMIDs sharing the same VA+ASID — a spec violation.
    IOVA pageAlignedVA = va & ~static_cast<IOVA>(PAGE_SIZE - 1u);
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->second.vmid == vmid &&
                it->second.iova == pageAlignedVA &&
                it->second.asid == asid) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateByVMIDAndVARange(uint16_t vmid, IOVA start, IOVA end, uint16_t asid) {
    // BUG-NEW-37 fix: ARM §4.4.2.3 CMD_TLBI_NH_VA (RIL path) — evict entries where
    // entry.vmid == vmid AND iova in [start,end] AND entry.asid == asid (joint match).
    IOVA pageAlignedStart = start & ~static_cast<IOVA>(PAGE_SIZE - 1u);
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            IOVA entryVA = it->second.iova;
            if (it->second.vmid == vmid &&
                entryVA >= pageAlignedStart && entryVA <= end &&
                it->second.asid == asid) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateByVMIDAndVA(uint16_t vmid, IOVA va) {
    // BUG-NEW-37 fix: ARM §4.4.2.4 CMD_TLBI_NH_VAA — evict entries where
    // entry.vmid == vmid AND entry.iova == va (any ASID, VMID-scoped).
    // Previously called invalidateByVA() which ignores VMID, evicting entries
    // from other VMIDs sharing the same VA — a spec violation.
    IOVA pageAlignedVA = va & ~static_cast<IOVA>(PAGE_SIZE - 1u);
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->second.vmid == vmid && it->second.iova == pageAlignedVA) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateEL2E2HByASID(uint16_t asid) {
    // BUG-NEW-D fix: ARM §4.4.2.10 CMD_TLBI_EL2_ASID — evict only EL2_E2H entries
    // matching ASID.  EL1_EL0 entries with the same ASID must be preserved.
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if (it->second.strw == StreamWorld::EL2_E2H && it->second.asid == asid) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateEL2ByVAAndASID(IOVA va, uint16_t asid) {
    // BUG-NEW-E fix: ARM §4.4.2.8 CMD_TLBI_EL2_VA — evict entries where
    // (strw == EL2 || strw == EL2_E2H) AND iova == va AND asid == asid.
    // EL1_EL0 entries at the same VA must be preserved.
    IOVA pageAlignedVA = va & ~static_cast<IOVA>(PAGE_SIZE - 1u);
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if ((it->second.strw == StreamWorld::EL2 || it->second.strw == StreamWorld::EL2_E2H) &&
                it->second.iova == pageAlignedVA && it->second.asid == asid) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateEL2ByVA(IOVA va) {
    // BUG-NEW-E fix: ARM §4.4.2.9 CMD_TLBI_EL2_VAA — evict entries where
    // (strw == EL2 || strw == EL2_E2H) AND iova == va (any ASID).
    // EL1_EL0 entries at the same VA must be preserved.
    IOVA pageAlignedVA = va & ~static_cast<IOVA>(PAGE_SIZE - 1u);
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            if ((it->second.strw == StreamWorld::EL2 || it->second.strw == StreamWorld::EL2_E2H) &&
                it->second.iova == pageAlignedVA) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateEL2ByVARange(IOVA start, IOVA end, uint16_t asid) {
    // BUG-NEW-E fix: ARM §4.4.2.8 CMD_TLBI_EL2_VA RIL path — evict entries where
    // (strw == EL2 || strw == EL2_E2H) AND iova in [start, end] AND asid matches.
    // EL1_EL0 entries in the same range must be preserved.
    IOVA pageAlignedStart = start & ~static_cast<IOVA>(PAGE_SIZE - 1u);
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        CacheStripe& stripe = stripes[i];
        auto it = stripe.list.begin();
        while (it != stripe.list.end()) {
            IOVA entryVA = it->second.iova;
            if ((it->second.strw == StreamWorld::EL2 || it->second.strw == StreamWorld::EL2_E2H) &&
                entryVA >= pageAlignedStart && entryVA <= end && it->second.asid == asid) {
                const CacheKey& key = it->first;

                auto& streamSet = stripe.streamIndex[key.streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(key.streamID);
                }

                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }

                stripe.map.erase(it->first);
                it = stripe.list.erase(it);
            } else {
                ++it;
            }
        }
    }
}

void TLBCache::invalidateStream(StreamID streamID) {
    // Per-stripe iteration: acquire only one stripe lock at a time
    // to allow concurrent operations on other stripes
    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        std::lock_guard<std::mutex> lock(lockStripes[i]);
        CacheStripe& stripe = stripes[i];

        auto streamIt = stripe.streamIndex.find(streamID);
        if (streamIt != stripe.streamIndex.end()) {
            // Invalidate all entries for this stream
            for (const auto& key : streamIt->second) {
                auto mapIt = stripe.map.find(key);
                if (mapIt != stripe.map.end()) {
                    stripe.list.erase(mapIt->second);
                    stripe.map.erase(mapIt);
                }

                // Remove from pasidIndex - O(1) with unordered_set
                StreamPASIDKey spKey{key.streamID, key.pasid};
                auto& pasidSet = stripe.pasidIndex[spKey];
                pasidSet.erase(key);
                if (pasidSet.empty()) {
                    stripe.pasidIndex.erase(spKey);
                }
            }
            stripe.streamIndex.erase(streamIt);
        }
    }
}

void TLBCache::invalidatePASID(StreamID streamID, PASID pasid) {
    // Per-stripe iteration: acquire only one stripe lock at a time
    // to allow concurrent operations on other stripes
    StreamPASIDKey spKey{streamID, pasid};

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        std::lock_guard<std::mutex> lock(lockStripes[i]);
        CacheStripe& stripe = stripes[i];

        auto pasidIt = stripe.pasidIndex.find(spKey);
        if (pasidIt != stripe.pasidIndex.end()) {
            // Invalidate all entries for this stream+pasid
            for (const auto& key : pasidIt->second) {
                auto mapIt = stripe.map.find(key);
                if (mapIt != stripe.map.end()) {
                    stripe.list.erase(mapIt->second);
                    stripe.map.erase(mapIt);
                }

                // Remove from streamIndex - O(1) with unordered_set
                auto& streamSet = stripe.streamIndex[streamID];
                streamSet.erase(key);
                if (streamSet.empty()) {
                    stripe.streamIndex.erase(streamID);
                }
            }
            stripe.pasidIndex.erase(pasidIt);
        }
    }
}

void TLBCache::invalidatePage(StreamID streamID, PASID pasid, IOVA iova,
                              SecurityState securityState) {
    // ARM §3.13: invalidation must match the security state of the entry.
    invalidate(streamID, pasid, iova, securityState);
}

void TLBCache::clear() {
    // Acquire all locks for full cache clear
    AllLocksGuard allLocks(*this);

    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        stripes[i].map.clear();
        stripes[i].list.clear();
        stripes[i].streamIndex.clear();
        stripes[i].pasidIndex.clear();
    }
}

// Statistics
uint64_t TLBCache::getHitCount() const {
    return hitCount.load(std::memory_order_relaxed);
}

uint64_t TLBCache::getMissCount() const {
    return missCount.load(std::memory_order_relaxed);
}

uint64_t TLBCache::getTotalLookups() const {
    return hitCount.load(std::memory_order_relaxed) + missCount.load(std::memory_order_relaxed);
}

double TLBCache::getHitRate() const {
    uint64_t hits = hitCount.load(std::memory_order_relaxed);
    uint64_t misses = missCount.load(std::memory_order_relaxed);
    uint64_t total = hits + misses;
    return total > 0 ? static_cast<double>(hits) / static_cast<double>(total) : 0.0;
}

size_t TLBCache::getSize() const {
    // Acquire all locks for accurate count
    AllLocksGuard allLocks(*this);

    size_t totalSize = 0;
    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        totalSize += stripes[i].list.size();
    }
    return totalSize;
}

size_t TLBCache::getCapacity() const {
    // maxSize is only modified under all locks, so we need all locks to read it safely
    AllLocksGuard allLocks(*this);
    return maxSize;
}

size_t TLBCache::getMaxSize() const {
    return getCapacity();
}

void TLBCache::resetStatistics() {
    hitCount.store(0, std::memory_order_relaxed);
    missCount.store(0, std::memory_order_relaxed);
}

void TLBCache::reset() {
    {
        // Acquire all locks for full reset
        AllLocksGuard allLocks(*this);

        for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
            stripes[i].map.clear();
            stripes[i].list.clear();
            stripes[i].streamIndex.clear();
            stripes[i].pasidIndex.clear();
        }
    }

    // Reset statistics (atomic operations don't need locks)
    hitCount.store(0, std::memory_order_relaxed);
    missCount.store(0, std::memory_order_relaxed);
}

// Configuration
void TLBCache::setMaxSize(size_t newMaxSize) {
    // Acquire all locks for configuration change
    AllLocksGuard allLocks(*this);

    maxSize = newMaxSize;

    // Calculate total current size
    size_t totalSize = 0;
    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        totalSize += stripes[i].list.size();
    }

    // Evict LRU entries across all stripes to meet global limit
    while (totalSize > maxSize) {
        // Find the stripe with the oldest (LRU) entry
        size_t oldestStripe = 0;
        uint64_t oldestTimestamp = UINT64_MAX;

        for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
            if (!stripes[i].list.empty()) {
                auto last = stripes[i].list.end();
                --last;
                if (last->second.timestamp < oldestTimestamp) {
                    oldestTimestamp = last->second.timestamp;
                    oldestStripe = i;
                }
            }
        }

        // Evict from stripe with oldest entry
        if (!stripes[oldestStripe].list.empty()) {
            auto last = stripes[oldestStripe].list.end();
            --last;
            const CacheKey& evictKey = last->first;

            auto& streamSet = stripes[oldestStripe].streamIndex[evictKey.streamID];
            streamSet.erase(evictKey);
            if (streamSet.empty()) {
                stripes[oldestStripe].streamIndex.erase(evictKey.streamID);
            }

            StreamPASIDKey spKey{evictKey.streamID, evictKey.pasid};
            auto& pasidSet = stripes[oldestStripe].pasidIndex[spKey];
            pasidSet.erase(evictKey);
            if (pasidSet.empty()) {
                stripes[oldestStripe].pasidIndex.erase(spKey);
            }

            stripes[oldestStripe].map.erase(evictKey);
            stripes[oldestStripe].list.erase(last);
            --totalSize;
        } else {
            break;  // Safety: no more entries to evict
        }
    }

    // Recalculate effectiveStripes and per-stripe max sizes using the same logic
    // as the constructor.  Updating the member is required so getLockStripeIndex()
    // routes new insertions to the correct (active) stripes after the resize.
    // BUG-1 fix: previously setMaxSize() blindly assigned to all NUM_LOCK_STRIPES
    // stripes, doubling effective capacity for small sizes vs. what getCapacity() reports.
    effectiveStripes = NUM_LOCK_STRIPES;
    if (maxSize < NUM_LOCK_STRIPES * 4) {
        effectiveStripes = (maxSize + 3) / 4;  // At least 4 entries per stripe
        if (effectiveStripes < 1) {
            effectiveStripes = 1;
        }
    }

    size_t entriesPerStripe = (maxSize + effectiveStripes - 1) / effectiveStripes;
    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        stripes[i].maxEntriesPerStripe = (i < effectiveStripes) ? entriesPerStripe : 0;
    }
}

// Helper methods
uint64_t TLBCache::getCurrentTimestamp() const {
    return std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
}

CacheKey TLBCache::makeKey(StreamID streamID, PASID pasid, IOVA iova, SecurityState securityState) const {
    CacheKey key;
    key.streamID = streamID;
    key.pasid = pasid;
    key.iova = iova;
    key.securityState = securityState;
    return key;
}

// Thread-safe atomic statistics snapshot
TLBCache::CacheStatistics TLBCache::getAtomicStatistics() const {
    CacheStatistics stats;

    stats.hitCount = hitCount.load(std::memory_order_relaxed);
    stats.missCount = missCount.load(std::memory_order_relaxed);
    stats.totalLookups = stats.hitCount + stats.missCount;
    stats.hitRate = stats.totalLookups > 0 ?
        static_cast<double>(stats.hitCount) / static_cast<double>(stats.totalLookups) : 0.0;

    // For size and maxSize, we need all locks briefly for accurate count
    {
        AllLocksGuard allLocks(*this);
        stats.currentSize = 0;
        for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
            stats.currentSize += stripes[i].list.size();
        }
        stats.maxSize = maxSize;
    }

    return stats;
}

// Lock striping helper methods
size_t TLBCache::getLockStripeIndex(const CacheKey& key) const {
    // Use the existing CacheKeyHash for consistent hashing.
    // Route to hash % effectiveStripes so that all entries land in active stripes
    // when the cache uses fewer than NUM_LOCK_STRIPES active stripes (small sizes).
    CacheKeyHash hasher;
    size_t hash = hasher(key);
    return hash % effectiveStripes;
}

std::mutex& TLBCache::getLockStripe(const CacheKey& key) const {
    size_t index = getLockStripeIndex(key);
    return lockStripes[index];
}

// RAII guard for acquiring all locks in order
TLBCache::AllLocksGuard::AllLocksGuard(const TLBCache& cache) : cache_(cache) {
    // Acquire locks in strict order to prevent deadlock
    for (size_t i = 0; i < NUM_LOCK_STRIPES; ++i) {
        cache_.lockStripes[i].lock();
    }
}

TLBCache::AllLocksGuard::~AllLocksGuard() {
    // Release in reverse order (not strictly necessary but good practice)
    for (size_t i = NUM_LOCK_STRIPES; i > 0; --i) {
        const_cast<std::mutex&>(cache_.lockStripes[i - 1]).unlock();
    }
}

}
