# The reference extensions against their standards

Date: 2026-10-23, against rlm v0.2.0. The five files in
`types/extensions/reference/` are marked ILLUSTRATIVE; this is the check
their headers ask for, clause by clause, with what each verdict rests on.
It changes no file: each finding is a candidate for a new major version of
its extension (a formula change is a major version), and the files stay
illustrative until those land. Only `rail.ebd` was checked against the
standard's own text; ISO and IEC texts are paywalled, so the others rest on
the secondary sources named in each section.

Files checked (this repository):
`types/extensions/reference/{iso13855,ebd,occlusion,pst,descent}.rlx`, together with
`docs/design/extension-examples.md` (the worked examples) and the domain survey in
`docs/design/extension-language-exploration.md` §3.2–3.7, where the standards and
formulas were first written down, each marked `[unverified]`.

## How much of this rests on the standard's own text

| Extension | Standard | Best source actually read | Primary text? |
|---|---|---|---|
| `rail.ebd` | ERA SUBSET-026-3 v4.0.0 §3.13 | **The SRS itself**: downloaded from ERA (`https://www.era.europa.eu/system/files/2023-09/index004_-_SUBSET-026_v400.zip`, `SUBSET-026-3 v400.pdf`, 05/07/2023), plus ERA_ERTMS_040026 v1.5, "Introduction to ETCS braking curves" (`https://www.era.europa.eu/system/files/2022-11/Introduction%20to%20ETCS%20braking%20curves.pdf`) | **Yes** |
| `medical.occlusion` | IEC 60601-2-24:2012 | IEC/VDE preview (front matter only); an IEEE PSES presentation that sets the Ed1.0 and Ed2.0 clause texts side by side; a 1998-edition test report; the Medfusion 3500 IFU; FDA's 2014 guidance | Partly: clause wording is quoted by secondary sources. The preview does not reach clause 201.12 |
| `process.pst` | IEC 61511-1:2016 | IChemE Hazards 26 paper quoting the definition; InstMC FS-SIG briefing note (Dearden, 2022); Kenexis; ORS Consulting | No, secondary sources only |
| `machine.iso13855` | ISO 13855:2010 (the 2024 edition is now current) | Vendor guides from ReeR and Keyence; Drives & Controls on the 2024 edition | No, secondary sources only (paywalled) |
| `uav.descent` | **None.** `extension-examples.md` names no standard. The exploration doc §3.6 names "EASA Specific category / SORA 2.5 (JARUS), ASTM F3548" and PX4 practice | PX4 parameter reference (`https://docs.px4.io/main/en/advanced_config/parameter_reference.html`) | PX4 docs are primary for PX4. No regulation is involved |

Every verdict below is labelled with the kind of source it rests on.

---

## 1. `machine.iso13855` (ISO 13855, ESPE minimum distance)

Sources:
- ReeR, "Determination of the safety distance": https://int.reersafety.com/academy/industrial-safety-guide/determination-of-the-safety-distance/ (cites ISO 13855:2010 Table 1)
- Keyence, "Light Curtain Installation and Safety Distance": https://keyence.com/ss/products/safetyknowledge/caution
- Datalogic catalogue p.382: https://www.datalogic.com/upload/Flipbook/General%20Catalogue%20S&S/files/assets/basic-html/page382.html (search snippet only)
- Drives & Controls on EN ISO 13855:2024: https://www.drivesandcontrols.ca/product-news/safety-standard-en-iso-13855
- Pilz's EN ISO 13855 page and the Leuze white paper returned HTTP 403. Only their search snippets were seen.

**Clause numbers are unverified in every source.** None of the vendor pages gives a clause number.

| Item in file | What the sources say | Verdict |
|---|---|---|
| `T = device_response + reaction_total` | `T = t1 + t2`: t1 is the protective device response time, t2 is the machine's stopping time (ReeR, Keyence: "Maximum time required by the machine to stop + response time of the light curtain") | MATCHES in form (secondary). **Caveat:** core's `reaction_total` is detection + route + settle. If a contract's core `detection` already includes the ESPE response, `device_response` is counted twice, which errs safe. The worked example has no core detection term, so nothing is double-counted there |
| `C = 8 * (d - 14 mm)` for d ≤ 40 mm | `C = 8 × (d − 14)` for hand and finger detection (ReeR: d = 14–40 mm; Keyence: d ≤ 40 mm, "with C ≥ 0") | MATCHES (secondary) |
| `C = 850 mm` for d > 40 mm | Keyence: 40 < d ≤ 70 uses **K = 1600 mm/s fixed** and C = 850. ReeR: arms and legs (d = 50–90 mm) and multi-beam access control: S = 1600·T + 850 | Numerically MATCHES, by accident. The file still runs the 2000-then-1600 rule on this branch, but 2000·T + 850 > 500 always holds, so the result is always 1600·T + 850. The branch is also unreachable, because `domain: detection_capability in [14 mm, 40 mm]`. It does not cover d > 70 (multi-beam, with prescribed beam heights) |
| `s_2000 = 2000 mm/s·T + C`; if ≤ 500 then `max(s_2000, 100 mm)` | K = 2000 mm/s; S ≥ 100 mm | MATCHES (secondary) |
| else `max(1600 mm/s·T + C, 500 mm)` | "If S > 500 mm, recalculate with K = 1600; the distance must in no case be lower than 500 mm" (ReeR). Keyence's ISO section says "≥ 500" here but uses "> 500" in its GL-R example | MATCHES (secondary). Note that the standard **permits** the 1600 recalculation ("may"); it does not require it. Always taking the smaller result is the permissive reading, and it is legitimate |
| `domain: d in [14, 40] mm` | Hand and finger formula range is 14–40 mm | MATCHES |
| Worked example: T = 207 ms, C = 128, 542 > 500 → 459.2 → 500 mm | Arithmetic re-checked | MATCHES |

**What the file leaves out that the standard requires** (all from secondary sources):
- **Approach direction.** The formula covers only the perpendicular (normal) approach. A parallel or horizontal approach uses `S = 1600·T + (1200 − 0.4·H)` with C ≥ 850 mm, `H = 15(d − 50)` and H ≤ 1000 mm. An angled approach (5°–85°) is treated as perpendicular above 30° and parallel below.
- **Reaching over the detection zone (C_RO, ISO 13855:2010 Table 1).** For a vertical field, compare C with C_RO and use the larger. The file never checks it, so for a short curtain it can understate S.
- **Multi-beam (d > 70) devices**, with their prescribed beam heights.
- **Edition.** ISO 13855:2024 (EN version "came into force in November 2024" per Drives & Controls) restructures the formula as `S = (K × T) + D_DS + Z`. D_DS is the largest applicable reaching-distance supplement, and Z is an application-dependent supplement. K is still 2000 mm/s (upper limbs) or 1600 mm/s (walking), S ≥ 100 mm, and the 500 mm rule is unchanged. A Leuze snippet (not read in full, 403) mentions supplements D_DT/D_DO/D_DU, and for 40–55 mm a supplement of `12·d − 272`. Whether `8(d − 14)` survives unchanged in 2024 is **UNVERIFIED**. The file and the docs implement the 2010 form and do not name the edition.
- The stopping time t2 must be a measured worst case, possibly with an allowance for brake wear (ANSI B11.19's brake-monitor term is the US analogue). The file accepts whatever settle core supplies.

**Overall:** the 2010 perpendicular, d ≤ 40 mm case matches every secondary source. It is not complete for ISO 13855: approach direction, C_RO and the 2024 edition are missing.

---

## 2. `rail.ebd` (ETCS emergency braking, SUBSET-026-3 §3.13)

**Since rlm v0.2.1, `rail.ebd 1.0.0`** addresses the differences below that
its arithmetic can carry: the dead time is `max(T_traction, T_be)`, not the
sum; the train accelerates through it (`acceleration`, A_est) from an
under-read speed (`speed_error`, V_ura), and braking starts from that
speed; the table is A_safe (`a_safe`, correction factors applied and the
worst gradient folded in, since a speed table cannot hold a gradient that
varies with distance); and the check targets the supervised location
(`distance_to_svl`). Still not modelled: the separate A_est2 during
T_berem (1.0.0 uses the one acceleration throughout, which errs safe),
Kt_int, T_be_reduced, and a gradient varying along the route. The table
below is the comparison as made against 0.1.0.

Primary source read: **SUBSET-026-3 v4.0.0**, §3.13.2, §3.13.6, §3.13.8 and §3.13.9 (text extracted from ERA's PDF). Context: ERA_ERTMS_040026 v1.5, which references SRS 3.6.0. Appendix A (A.3.1 fixed values, A.3.12 T_be_reduced) is not in the downloaded zip and was **not read**.

| Item in file | SUBSET-026-3 v4.0.0 | Verdict |
|---|---|---|
| `a_ebd : table Velocity -> Acceleration`, "each entry holds from its speed up to the next" | §3.13.2.2.3.1.1–.4: deceleration is "a step function of the speed"; "up to seven steps"; `A_brake = AD_0 when 0 ≤ speed ≤ V1, AD_1 when V1 < speed ≤ V2 …`; "the last step … open ended" | MATCHES in shape. Bands are right-closed in the SRS (`V1 < v ≤ V2`) while the file says "from its speed up to the next". The difference sits on a measure-zero boundary and does not change the integral. The file has no seven-step cap |
| What `a_ebd` represents | §3.13.6.2.1.3/.4: the EBD uses `A_safe(V,d) = A_brake_safe(V,d) + A_gradient(d)` (or `MIN(A_brake_safe, A_MAXREDADH) + A_gradient` under reduced adhesion), and `A_brake_safe = Kdry_rst(V, M_NVEBCL) · (Kwet_rst(V) + M_NVAVADH·(1 − Kwet_rst(V))) · A_brake_emergency(V,d)` for gamma trains, or `Kv_int(V)·Kr_int(L_TRAIN)·A_brake_emergency(V)` for lambda trains (conversion model) | **DIFFERS / underspecified.** The key's doc string says "emergency deceleration" and does not say whether it is the nominal `A_brake_emergency` or the guaranteed `A_brake_safe`. Supplying the nominal value is **unsafe**: Kdry_rst and Kwet_rst are below 1. Supplying an already-corrected value would be the ETCS-conformant use. The file models no correction factor, no national value (M_NVEBCL, M_NVAVADH), no A_MAXREDADH, and **no gradient term** (§3.13.4, with rotating-mass compensation). A downhill gradient lowers A_safe, so leaving it out is unsafe on falling track |
| `traction_cut_off` | §3.13.2.2.2.1: `T_traction_cut_off` runs from the cut-off or EB command (t0) to A_traction = 0. §3.13.9.3.2.3: `T_traction = MAX(T_traction_cut_off − (T_warning + T_bs2); 0)` when the cut-off is implemented, else `T_traction = T_traction_cut_off` | Partly MATCHES. The file takes the "not implemented" case and ignores the warning-limit credit |
| `brake_build_up` | §3.13.2.2.3.2.4: equivalent build-up time `T_brake_build_up = T_brake_react + 0.5·T_brake_increase`. §3.13.6.2.2.3: `T_be = T_brake_emergency` (or `Kt_int · T_brake_emergency` in the conversion model). The EBI uses `T_be_reduced` (Appendix A.3.12, not read) | MATCHES if the contract supplies the equivalent time. The key's doc string does not say it must be the equivalent one rather than the raw time to full force. `Kt_int` is missing |
| `settle.duration = traction_cut_off + brake_build_up + Σ(Δv / a)` and `stopping_distance = v0·(traction_cut_off + brake_build_up) + Σ (hi² − lo²)/(2a)` | §3.13.9.3.2.2: the time from EB intervention to full braking effort "shall be split into two parts": T_traction, and `T_berem = MAX(T_be_reduced − T_traction; 0)` (§3.13.9.3.2.6). Both intervals start at the same command, so the dead time is **max(T_traction, T_be_reduced)**, not their sum. §3.13.9.3.2.10: `D_bec = max(V_est + V_delta0 + V_delta1/2, V_target)·T_traction + (max(V_est + V_delta0 + V_delta1, V_target) + V_delta2/2)·T_berem`, with `V_delta0 = V_ura` (speed under-reading), `V_delta1 = A_est1·T_traction` (measured acceleration ≥ 0), and `V_delta2 = A_est2·T_berem` (0 ≤ A_est2 ≤ 0.4 m/s²). The EBD is then entered at `V_bec`, not at v0 | **DIFFERS.** (a) The file **adds** the two times where the SRS overlaps them. That overestimates time and distance, which errs safe but is not the SRS model: with the example's 1.0 s and 2.5 s, the SRS gives 2.5 s of dead time, not 3.5 s. (b) The file holds speed constant through the dead time. The SRS lets the train keep **accelerating**, and it adds the speed measurement error V_ura. Both make the SRS distance longer than the file's when the train is accelerating, so the file is **unsafe** there. (c) The braking integral starts at v0 instead of `V_bec` |
| Σ (Δv/a) over the bands | The EBD is "composed of interconnected arcs of parabola" (§3.13.8.1.3). The time and distance over each constant-deceleration arc are textbook kinematics | MATCHES as kinematics, for a level track with A_safe constant per speed band. Gradient changes with **distance**, which a speed-only table cannot express |
| `check stop_before_eoa: stopping_distance <= distance_to_eoa` | §3.13.8.3.2: the EBD reaches zero speed at the **SvL** (Supervised Location, beyond the EOA by the overlap or danger point). §3.13.8.4.1: the **EOA** is supervised with the **SBD**, built on the *expected* service-brake deceleration `A_expected = A_brake_service + A_gradient`, not the emergency brake | **DIFFERS** conceptually. In ETCS, emergency braking is supervised against the SvL. Checking it against the EOA is stricter and errs safe, but it is not the ETCS supervision rule. The key name `distance_to_eoa` would mislead an ETCS engineer |
| `domain: a_ebd in (0, 5] m/s²` | No such bound in the SRS | Not from the standard (illustrative). Harmless |
| Worked example (160 km/h, 1600 m: 155.56 + 514.40 + 859.79 = 1529.75 m; 64.35 s) | Re-computed | Arithmetic MATCHES the file's own model |

**Left out:** the A_safe correction chain (Kdry_rst with the EBCL confidence level, Kwet_rst, M_NVAVADH, or Kv_int/Kr_int/Kt_int for lambda trains), gradient with rotating mass, reduced adhesion, special-brake availability by location (`A_brake_emergency(V,d)`), estimated acceleration during build-up, V_ura, T_be_reduced, and SvL versus EOA. The file's own comment says "ETCS braking-curve style", which is accurate: it is a simplification, not SUBSET-026.

---

## 3. `medical.occlusion` (IEC 60601-2-24, time to occlusion alarm)

Sources:
- IEC 60601-2-24:2012 preview (VDE): https://www.vde-verlag.de/iec-normen/preview-pdf/info_iec60601-2-24%7Bed2.0%7Db.pdf. Read: the contents list Figure 201.112 "Test apparatus to determine the OCCLUSION ALARM THRESHOLD and BOLUS volumes". The occlusion clause itself is beyond the preview.
- Liem Lam (certifier), "Differences Between IEC 60601-2-24 Edition 1.0 and 2.0", IEEE PSES: https://ewh.ieee.org/r6/ocs/pses/IEC%2060601-2%2024%20standard%20update%20requirements%20presentation.pdf. Quotes clause text from both editions. Secondary, but a clause-by-clause reproduction.
- Guangzhou MIQSIC test report against IEC 60601-2-24:1998 (tender document): https://storage.mtender.gov.md/get/58c140a5-ff0f-4b31-b2e9-349628c50ab1-1561446906258
- ICU Medical Medfusion 3500 V6 operator's manual: https://www.icumed.com/media/la0n3opi/mf-3500-v6-op-manual.pdf
- FDA, Infusion Pumps Total Product Life Cycle (2014): https://www.fda.gov/media/78369/download

What the standard requires, per the clause text quoted in those sources:
- **201.7.9.2.101** (Ed2; Ed1 6.8.2): the instructions for use shall state "the maximum time for activation of the occlusion alarm when the ME EQUIPMENT is operating at the MINIMUM RATE, INTERMEDIATE RATE and the MINIMUM SELECTABLE RATE and at the minimum and maximum selectable OCCLUSION ALARM THRESHOLD", and that "temperature and length of ADMINISTRATION SET affect the time". They shall also state the BOLUS volume generated at the INTERMEDIATE RATE on reaching the minimum and maximum threshold (Ed1 wording, per the 1998 test report).
- **201.3.211** MINIMUM RATE: "lowest rate selectable by the OPERATOR, but not less than 1 ml/h". **201.3.213** MINIMUM SELECTABLE RATE: lowest selectable rate "if lower than the MINIMUM RATE". **201.3.208** INTERMEDIATE RATE: 25 ml/h for volumetric pumps, **5 ml/h for syringe or container pumps**.
- **201.12.4.4.104**: means shall protect the patient from under-infusion resulting from occlusion. An acceptable method is to activate a HIGH PRIORITY alarm signal at the threshold and terminate flow. The test uses the INTERMEDIATE RATE and the minimum threshold, and is repeated with any automatic bolus-reduction feature disabled.
- The standard sets **no maximum time to alarm**. It requires the time to be measured (Fig. 201.112) and disclosed. FDA likewise asks for "time to occlusion alarm" among the device specifications and sets no number.

| Item in file | What the sources say | Verdict |
|---|---|---|
| `detection = (alarm_threshold − baseline_pressure)·compliance / min_rate + signal_delay` | The standard gives **no formula**. It measures the time on a test rig. The linear lumped model `Δp·C/Q` is physics (rate of pressure rise = Q/C) and is consistent with manufacturers' qualitative statements: the Medfusion IFU names "occlusion pressure setting, flow rate setting, syringe and tubing compliance" as the three factors | UNVERIFIABLE as a clause of the standard, which contains no such clause. As physics it is a first-order approximation: real compliance is pressure-dependent, and start-up mechanical slack adds time. The Medfusion table is "Max time to occlusion (Start-up)" (for example 31 min at 1 mL/h, 16 psi, 20 mL syringe). The model can underestimate a start-up occlusion |
| `worst_case: decreasing in min_rate` (worst at the lowest rate) | Medfusion's measured table: 1 mL/h → 10–31 min; 5 mL/h → 1–6 min. The standard requires disclosure at MINIMUM and MINIMUM SELECTABLE rates for this reason | MATCHES in direction (manufacturer data) |
| `min_rate` key, "lowest programmable rate covered" | Ed2 distinguishes MINIMUM RATE (≥ 1 ml/h) from MINIMUM SELECTABLE RATE (may be below 1 ml/h, for example 0.1 ml/h) and requires disclosure at both | DIFFERS in a way that matters: if a contract writes 1 mL/h (as the example does) for a pump selectable down to 0.1 mL/h, the derived "worst case" is not the worst case. The key should be the minimum **selectable** rate |
| `domain: min_rate in (0, 1200] mL/h` | No such bound in the standard | Illustrative |
| `signal_delay` ("alarm signal generation delay") | The term is from IEC 60601-1-8. Only a secondary reproduction exists, per the exploration doc. Not re-verified here | UNVERIFIED |
| `report bolus_on_release = compliance·(threshold − baseline)` | The standard measures the bolus at the INTERMEDIATE rate on reaching the minimum and maximum thresholds, with bolus reduction enabled and disabled. It gives no formula | UNVERIFIABLE as a clause. As physics it is the stored volume, an upper bound only if no anti-bolus back-off runs |
| Example numbers (0.003 mL/mmHg, 300 mmHg, 1 mL/h → 50.4 min) | Medfusion's measured 31 min at 1 mL/h, 827 mmHg, 20 mL syringe. Plausible order of magnitude | Example values only |

**Left out:** the standard's own test points (minimum selectable rate, intermediate rate 5 or 25 mL/h, minimum and maximum thresholds), the dependence on administration-set length and temperature that the IFU must state, HIGH PRIORITY alarm plus flow stop as the reaction, and anti-bolus behaviour. The file's "IEC 60601-2-24 style" label is fair. The standard supplies no arithmetic to verify against: it is measure-and-disclose.

---

## 4. `process.pst` (IEC 61511, process safety time)

Sources (all secondary; IEC 61511-1 itself was not read):
- IChemE Hazards 26, paper 16, "Determination of Alarm Safety Response Time" (Amec Foster Wheeler, 2016): https://www.icheme.org/media/11753/hazards-26-paper-16-determination-of-alarm-safety-response-time.pdf. Read in full.
- InstMC FS-SIG Briefing Note, H. T. Dearden, "Specifying SIF Response Time" (20 Sep 2022): https://www.instmc.org/_userfiles/pages/groupfiles/functional_safety/fssig_bn_sif_response_time.pdf. Read in full.
- Kenexis, "Determining Process Safety Time (now MERT)": https://www.kenexis.com/?p=25532
- ORS Consulting: https://www.ors-consulting.com/difference-between-the-process-safety-time-and-the-sif-response-time
- Search snippets cite the definition as IEC 61511-1:2016 **3.2.52.1**, and **10.3.2** for the SRS response-time requirement. Neither clause number was confirmed from the text. The exploration doc's "3.2.52" is close.

Definition, as quoted by IChemE from BS EN 61511:2004: PST is "the time period between a failure occurring in the process or the basic process control system (with the potential to give rise to hazardous event) and the occurrence of a hazardous event if the safety instrumented function is not performed."

| Item in file | What the sources say | Verdict |
|---|---|---|
| `hazard.ftti = (hazard_pressure − trip_setpoint) / max_rise_rate`, labelled as the PST | The standard's PST starts at the **initiating failure**, not at the trip point (definition above; ORS: "the total time from the initial valve failure first occurring"). Practitioners usually compute the time **from trip set-point to hazard**, which is the window actually available to the SIF. IChemE eq. (1): `PST = (PV_haz − PV_sp) / (dPV/dt)`, "Time between alarm set point and hazardous event". Dearden: "I here use the term PST to mean the time from SIF trip point to hazard consequence … NOT the time from initiating event" | The formula MATCHES common practice (IChemE eq. 1, Dearden). It DIFFERS from the standard's literal definition: what the file derives is the set-point-to-hazard window, which is shorter than PST as defined. Kenexis says this ambiguity is why the next edition replaces PST with **MERT** (maximum event response time). Calling the derived number "the PST" is imprecise. Using it as the SIF deadline is the right engineering choice |
| Linear rise at `max_rise_rate` | IChemE: "Estimated rate of change of process variable". In practice it is checked by dynamic simulation (HYSYS) because rises are rarely linear | First-order approximation. `worst_case: decreasing in max_rise_rate` is correct only for a linear model |
| `settle.duration = valve_stroke`; core then checks detection + route + settle ≤ ftti | The SIF response time is sensor + logic solver + final element (Cenosco/PTQ snippets). Dearden's design requirement: `RT_S@MT + RT_L@MT + RT_FE@MT < PST − PTD`, where PTD is the **potential trip delay from trip-setting tolerance** and each RT is the response time **at the end of the mission time**, after degradation | Structure MATCHES (sum of sensor, logic and final-element times). Leaves out: the trip-tolerance deduction (PTD), response-time **growth** over the mission time (fouling, seal wear), and the **process lag after the valve closes** (pressure can keep rising, so the hazard can still occur after the stroke completes) |
| "SIF response ≤ PST/2" practice | Dearden: "The commonly cited 'rule of thumb' … is that it should be less than 50% of the process safety time". He calls it "arbitrary" and shows it is equivalent to 7%/yr exponential growth over a 10-year mission time. No source places the 50% rule in IEC 61511 itself. The exploration doc attributes it to ANSI/ISA 84 commentary, which remains unconfirmed | The file **does not apply** any margin: core requires only total ≤ ftti. In the worked example 2.40 s against 4.00 s passes, and so would 3.9 s. **Leaves out** the customary 50% margin, or a mission-time growth margin |
| `domain: hazard_pressure > trip_setpoint, max_rise_rate > 0` | Physical necessities | MATCHES |
| Example (11 − 9 bar)/0.5 bar/s = 4 s | Arithmetic | MATCHES (illustrative constants) |

---

## 5. `uav.descent` (lost-link land and battery reserve)

**No standard or regulation is named** in `extension-examples.md` or in the `.rlx` header. The exploration doc §3.6 lists EASA Specific category / SORA 2.5 (JARUS) and ASTM F3548 as domain sources, and says the vocabulary comes from "autopilot practice (PX4, ArduPilot)". Neither SORA nor F3548 was found to contain a descent-time or energy-reserve formula, and none was looked for in their texts, since both are paywalled or long and neither is cited for a formula. The formulas are therefore checked against **PX4**, the stated source of the model. PX4 is an implementation, not a standard.

Source: PX4 parameter reference (main), read directly: https://docs.px4.io/main/en/advanced_config/parameter_reference.html

| Item in file | PX4 | Verdict |
|---|---|---|
| Two-segment descent: `(h − h_slow)/v_desc + h_slow/v_land` | `MPC_LAND_ALT1` (default 10 m): "Below this altitude descending velocity gets limited to a value **between** MPC_Z_VEL_MAX_DN (or MPC_Z_V_AUTO_DN) and MPC_LAND_SPEED". `MPC_LAND_ALT2` (default 5 m): "Below this altitude descending velocity gets limited to MPC_LAND_SPEED". `MPC_LAND_ALT3` (1 m) with `MPC_LAND_CRWL` (0.3 m/s) below it when a distance sensor is valid | DIFFERS. PX4 has **three** zones: full speed above ALT1, a **ramp** between ALT1 and ALT2, land speed below ALT2, plus an optional crawl below ALT3. With `slow_altitude = ALT1` the file's model is conservative (longer). With `slow_altitude = ALT2` it is optimistic, since it ignores the ramp. With a range finder, the crawl at 0.3 m/s below 1 m adds time the file omits |
| `descent_speed` (example 3 m/s) | `MPC_Z_V_AUTO_DN`, descent velocity in autonomous modes, default 1.5 m/s, range 0.5–4 | Parameter exists. The example value is not the default |
| `land_speed` (example 0.7 m/s) | `MPC_LAND_SPEED`, "Landing descend rate". The page lists 0.6 and 0.7 m/s in the min/default columns | MATCHES the PX4 default (0.7) |
| Detection = lost-link timeout (core) | `COM_DL_LOSS_T` default 10 s (range 5–300), then `NAV_DLL_ACT` ∈ {Hold, Return, Land, Terminate, Disarm}, default 0 = disabled. **`COM_FAIL_ACT_T`**: "Delay between failsafe condition triggered and failsafe reaction … wait in Hold mode" | Leaves out `COM_FAIL_ACT_T`, the hold delay before the action, which adds to the reaction time. It also assumes the configured action is Land. If it is Return (RTL), the settle must include return distance and climb, which the exploration doc mentions and the file does not model |
| `energy_reserve: hover_power · reaction_total ≤ capacity · (trigger_soc − reserve_soc)` | PX4 battery thresholds `BAT_LOW_THR` 0.15, `BAT_CRIT_THR` 0.07 ("commonly will trigger Return"), `BAT_EMERGEN_THR` 0.05 ("commonly will trigger landing"). `COM_FLTT_LOW_ACT` compares remaining flight time with the time to return. There is no PX4 or regulatory formula of this form | UNVERIFIABLE against any standard: it is the author's own predicate. Assumptions to state: (a) energy linear in state of charge, ignoring voltage sag, temperature and the end-of-discharge cliff; (b) hover power throughout, although during the detection interval the vehicle is still flying its mission, at cruise or climb power, which may exceed hover; (c) wind is ignored. The example's 0.30 and 0.22 are not PX4 defaults (0.15, 0.07, 0.05) |
| Domains (speeds > 0, land ≤ descent, altitudes > 0, power > 0) | Physical. `land_speed ≤ descent_speed` mirrors PX4's ordering | MATCHES (physics) |
| Example: 110/3 + 10/0.7 = 50.95 s; 600 W × 60.95 s = 10.16 Wh ≤ 17.76 Wh | Arithmetic | MATCHES |

---

## Summary of verdicts

| Extension | Core formula | Main gaps or differences |
|---|---|---|
| iso13855 | MATCHES ISO 13855:2010, perpendicular approach, d ≤ 40 (secondary sources) | No parallel or angled approach, no C_RO (Table 1), no multi-beam; the 2024 edition is restructured (`K·T + D_DS + Z`); possible double count of the ESPE response |
| rail.ebd | Kinematics correct; differs from the SRS | T_traction and T_be are **summed** where the SRS **overlaps** them (safe side); no acceleration during build-up and no V_ura (unsafe side); no Kdry/Kwet/national values, no gradient (unsafe if `a_ebd` is nominal); supervised against the EOA where the SRS uses the SvL for EBD and the SBD for the EOA |
| occlusion | No formula in the standard to verify; the physics direction is confirmed by manufacturer data | Should cover the MINIMUM **SELECTABLE** rate (may be < 1 mL/h); the standard is measure-and-disclose at fixed test points; start-up slack and non-linear compliance are not modelled |
| pst | MATCHES common practice (set-point-to-hazard ÷ rate) | Not the standard's literal PST (which starts at the initiating failure; MERT is coming); no trip tolerance, no response-time growth, no post-closure process lag, no customary 50% margin |
| descent | No standard. Checked against PX4 | PX4 has a ramp zone and a crawl zone, and a `COM_FAIL_ACT_T` hold delay; RTL is not modelled; the energy predicate is the author's own, with hover power assumed during detection |
