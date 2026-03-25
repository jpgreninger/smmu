  BUG-QA-11 — Moderate | §5.2/§13.3 STE bypass output attributes                                                                                                                     
                                                                                                                                                                                     
  C++ only (Rust correct)                                                                                                                                                            
                                                                                                                                                                                     
  When STE.Config=0b100 (all-bypass), the spec requires STE output attribute fields (MTCFG/MemAttr, ALLOCCFG, SHCFG, NSCFG, PRIVCFG, INSTCFG) to be applied to bypass transactions.  
  C++ constructs a bare TranslationData with all output fields at zero. Rust correctly calls apply_output_attrs().                                                                   
                                                                                                                                                                                     
  - Fix: Apply the applyOutputAttrs lambda (already defined in stream_context.cpp) to the bypass result in smmu.cpp:1619-1621.                                                       
  - Files: cpp/src/smmu/smmu.cpp (~line 1619)
                                                                                                                                                                                     
  ---                                                             
  BUG-QA-12 — Moderate | §5.2/§5.5 STE.S2S (Stage-2 Stall) not applied                                                                                                               
                                                                                                                                                                                     
  Both C++ and Rust                                                                                                                                                                  
                                                                                                                                                                                     
  STE.S2S controls stall-vs-terminate for stage-2 faults independently of the stage-1 CD.S setting. C++ stores S2S but the stall decision at smmu.cpp:702 always uses the single     
  global FaultMode (which mirrors CD.S), not S2S. Rust doesn't store or use S2S at all. Also missing: validation that STALL_MODEL==0b01 && S2S==1 → C_BAD_STE.                       
                                                                                                                                                                                     
  - Fix: C++ — use S2S to drive the stall decision for stage-2 faults. Rust — add s2s field to StreamConfig, validate against STALL_MODEL, use it for stage-2 fault path.            
  - Files: cpp/src/smmu/smmu.cpp (~line 702); rust/smmu/src/types/config.rs, rust/smmu/src/smmu/mod.rs                                                                               
                                                                                                                                                                                     
  ---                                                                                                                                                                                
  BUG-QA-13 — Low | §5.2/§5.5 STE.S2R (Stage-2 Record) field missing                                                                                                                 
                                                                                                                                                                                     
  Both C++ and Rust
                                                                                                                                                                                     
  STE.S2R controls whether stage-2 faults are recorded in the event queue (analogous to CD.R for stage-1). When S2R=0 and S2S=0, stage-2 fault events must be suppressed. Neither    
  implementation stores S2R — stage-2 events are always recorded.
                                                                                                                                                                                     
  - Fix: Add s2r/s2R to StreamConfig. In the stage-2 fault recording path, suppress the event when S2R=0 && S2S=0.                                                                   
  - Files: cpp/include/smmu/types.h (StreamConfig), cpp/src/smmu/smmu.cpp (fault paths); rust/smmu/src/types/config.rs, rust/smmu/src/smmu/mod.rs
                                                                                                                                                                                     
  ---                                                             
  BUG-QA-14 — Low | §4.4.4.1 CMD_TLBI_NSNH_ALL over-invalidates                                                                                                                      
                                                                                                                                                                                     
  Both C++ and Rust
                                                                                                                                                                                     
  CMD_TLBI_NSNH_ALL should only invalidate Non-Secure Non-Hyp entries (excluding NS-EL2 and NS-EL2-E2H entries). Both implementations group it with TLBI_NH_ALL and call             
  invalidate_all() — a full flush. Functionally safe (superset) but spec non-conformant: it spuriously evicts EL2/VHE entries that should be preserved.
                                                                                                                                                                                     
  - Fix: Implement a scoped invalidation that excludes entries tagged StreamWorld==NS-EL2 and NS-EL2-E2H.                                                                            
  - Files: cpp/src/smmu/smmu.cpp (~lines 3925-3929); rust/smmu/src/smmu/mod.rs (~lines 5496-5499)
                                                                                                                                                                                     
  ---                                                             
  Also Verified as Correct
                                                                                                                                                                                     
  GBPA.ABORT handling, all three S1DSS values, EATS field, CMD_TLBI_S2_IPA TTL (hint only per spec), STAG=0 for non-stall events, translation output attributes (non-bypass paths).
                                                                                                                                                                                     
  ---                                                             
  Summary                                                                                                                                                                            
                                                                  
  ┌───────────┬──────────┬────────────────────────────────────┬────────────┐
  │    ID     │ Severity │                Spec                │  Affects   │                                                                                                         
  ├───────────┼──────────┼────────────────────────────────────┼────────────┤
  │ BUG-QA-11 │ Moderate │ §5.2/§13.3 STE bypass output attrs │ C++ only   │                                                                                                         
  ├───────────┼──────────┼────────────────────────────────────┼────────────┤
  │ BUG-QA-12 │ Moderate │ §5.5 STE.S2S stage-2 stall         │ C++ + Rust │                                                                                                         
  ├───────────┼──────────┼────────────────────────────────────┼────────────┤                                                                                                         
  │ BUG-QA-13 │ Low      │ §5.5 STE.S2R stage-2 record        │ C++ + Rust │                                                                                                         
  ├───────────┼──────────┼────────────────────────────────────┼────────────┤                                                                                                         
  │ BUG-QA-14 │ Low      │ §4.4.4.1 TLBI_NSNH_ALL scope       │ C++ + Rust │
  └───────────┴──────────┴────────────────────────────────────┴────────────┘                                                                                                         
   
  Awaiting your confirmation to proceed with fixes.                                                                                                                                  
                                                                  

