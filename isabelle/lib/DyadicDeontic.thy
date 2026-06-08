theory DyadicDeontic
  imports Mediator
begin

text \<open>
  \<^bold>\<open>DyadicDeontic\<close> — conditional obligation \<open>O\<langle>p \<bar> c\<rangle>\<close> and contrary-to-duty.

  @{theory_text Mediator}'s obligation operator is \<^emph>\<open>monadic\<close>:
  \<open>O\<langle>p\<rangle> \<equiv> (\<not> p \<longrightarrow> Viol)\<close>. It is honest about its own ceiling (see its lineage
  note): being context-free, it inherits the gentle-murderer / contrary-to-duty
  collapse. It cannot say \<^emph>\<open>"given the promise was broken, an acknowledgment is now
  owed"\<close> without that reducing to an \<^emph>\<open>unconditional\<close> duty to acknowledge.

  But contrary-to-duty — \<^bold>\<open>what is owed once a duty has already been breached\<close> — is
  exactly the formal home of \<^emph>\<open>repair, apology, making-it-right\<close>: the spine of every
  reconciliation a mediator actually steers. So the kernel needs a \<^emph>\<open>dyadic\<close>
  obligation that survives it.

  We give the standard preference / "best-antecedent-worlds" semantics (Hansson;
  Aqvist; Carmo & Jones), shallow-embedded in HOL in the LogiKEy / Benzmueller
  tradition: a \<^emph>\<open>betterness\<close> preorder on worlds, and

      \<open>O\<langle>p \<bar> c\<rangle>  \<equiv>  p holds throughout the betterness-best c-worlds.\<close>

  Conditioning on \<open>c\<close> is what lets the obligation \<^emph>\<open>see context\<close>: the duty in the
  already-sub-ideal "you broke it" worlds can differ from the duty overall, with no
  explosion. The kernel still only \<^emph>\<open>certifies the structure\<close> — whether a duty was
  breached, and what counts as repair, remain value questions handed back \<open>Unknown\<close>.
\<close>

section \<open>The operator, over an arbitrary betterness preorder\<close>

text \<open>Propositions are world-predicates \<open>'w \<Rightarrow> bool\<close>; \<open>R w v\<close> reads "w is at least
  as good as v". We keep the operator polymorphic in \<open>R\<close> so it is reusable, and prove
  the scenario facts against a concrete ranking below.\<close>

definition pstrict :: "('w \<Rightarrow> 'w \<Rightarrow> bool) \<Rightarrow> 'w \<Rightarrow> 'w \<Rightarrow> bool" where
  "pstrict R w v \<longleftrightarrow> R w v \<and> \<not> R v w"

text \<open>@{term optimal}: the \<open>R\<close>-maximal \<open>c\<close>-worlds — those \<open>c\<close>-worlds with no strictly
  better \<open>c\<close>-world. (Maximality, not optimality: robust even when betterness is partial.)\<close>
definition optimal :: "('w \<Rightarrow> 'w \<Rightarrow> bool) \<Rightarrow> ('w \<Rightarrow> bool) \<Rightarrow> 'w \<Rightarrow> bool" where
  "optimal R c w \<longleftrightarrow> c w \<and> \<not> (\<exists>v. c v \<and> pstrict R v w)"

text \<open>Conditional obligation: \<open>p\<close> holds throughout the best \<open>c\<close>-worlds.\<close>
definition cobl :: "('w \<Rightarrow> 'w \<Rightarrow> bool) \<Rightarrow> ('w \<Rightarrow> bool) \<Rightarrow> ('w \<Rightarrow> bool) \<Rightarrow> bool" where
  "cobl R p c \<longleftrightarrow> (\<forall>w. optimal R c w \<longrightarrow> p w)"

subsection \<open>Sanity laws (any betterness relation)\<close>

text \<open>Given \<open>c\<close>, \<open>c\<close> itself is obligatory — the best \<open>c\<close>-worlds are \<open>c\<close>-worlds.\<close>
lemma cobl_identity: "cobl R c c"
  by (auto simp: cobl_def optimal_def)

text \<open>Obligation is monotone in its consequent (the K-flavour, now \<^emph>\<open>relativised\<close>).\<close>
lemma cobl_mono:
  assumes "\<And>w. p w \<Longrightarrow> q w" and "cobl R p c"
  shows   "cobl R q c"
  using assms by (auto simp: cobl_def)

text \<open>And it distributes over conjunction of consequents.\<close>
lemma cobl_conj: "cobl R (\<lambda>w. p w \<and> q w) c \<longleftrightarrow> cobl R p c \<and> cobl R q c"
  by (auto simp: cobl_def)

section \<open>A contrary-to-duty scenario the monadic operator cannot survive\<close>

text \<open>
  Chisholm's set, in the register of a broken promise and its repair:

    (1) you ought to keep your word;
    (2) if you do \<^emph>\<open>not\<close> keep it, you ought to acknowledge / make repair;
    (3) in fact, the word was not kept.

  Under monadic SDL this set is the classic paradox: (2) becomes an unconditional
  \<open>O\<langle>ack\<rangle>\<close>, indistinguishable from "acknowledge no matter what", and the duties
  tangle. We show the \<^emph>\<open>dyadic\<close> reading is consistent and gives the intuitive answers:
  the acknowledgment is owed \<^bold>\<open>in the breach worlds and only there\<close>.
\<close>

datatype world = KeptAck | KeptSilent | BrokeAck | BrokeSilent

text \<open>A betterness ranking: keeping your word is ideal; \<^emph>\<open>given\<close> a breach, acknowledging
  is better than staying silent. (3 = ideal, 1 = worst.)\<close>
fun wrank :: "world \<Rightarrow> nat" where
  "wrank KeptAck     = 3"
| "wrank KeptSilent  = 3"
| "wrank BrokeAck    = 2"
| "wrank BrokeSilent = 1"

fun kept :: "world \<Rightarrow> bool" where
  "kept KeptAck = True" | "kept KeptSilent = True"
| "kept BrokeAck = False" | "kept BrokeSilent = False"

fun ack :: "world \<Rightarrow> bool" where
  "ack KeptAck = True"  | "ack KeptSilent = False"
| "ack BrokeAck = True" | "ack BrokeSilent = False"

definition bett :: "world \<Rightarrow> world \<Rightarrow> bool" where
  "bett w v \<longleftrightarrow> wrank v \<le> wrank w"

text \<open>\<open>bett\<close> is a genuine betterness preorder (reflexive + transitive).\<close>
lemma bett_refl: "bett w w" by (simp add: bett_def)
lemma bett_trans: "bett w v \<Longrightarrow> bett v u \<Longrightarrow> bett w u" by (simp add: bett_def)

lemma wrank_le3:    "wrank w \<le> 3"            by (cases w) auto
lemma broke_le2:    "\<not> kept v \<Longrightarrow> wrank v \<le> 2" by (cases v) auto
lemma pstrict_bett: "pstrict bett v w \<longleftrightarrow> wrank w < wrank v"
  by (auto simp: pstrict_def bett_def)

text \<open>The best worlds overall are exactly the ideal (kept-your-word) ones.\<close>
lemma optimal_top: "optimal bett (\<lambda>_. True) w \<longleftrightarrow> wrank w = 3"
proof
  assume "optimal bett (\<lambda>_. True) w"
  hence "\<not> wrank w < wrank KeptAck"
    by (auto simp: optimal_def pstrict_bett dest!: spec[where x = KeptAck])
  thus "wrank w = 3" using wrank_le3[of w] by simp
next
  assume w3: "wrank w = 3"
  have "\<not> (\<exists>v. True \<and> pstrict bett v w)"
  proof
    assume "\<exists>v. True \<and> pstrict bett v w"
    then obtain v where "wrank w < wrank v" by (auto simp: pstrict_bett)
    with w3 wrank_le3[of v] show False by simp
  qed
  thus "optimal bett (\<lambda>_. True) w" by (simp add: optimal_def)
qed

text \<open>The best \<^emph>\<open>breach\<close> world is the one in which you acknowledged.\<close>
lemma optimal_broke: "optimal bett (\<lambda>w. \<not> kept w) w \<longleftrightarrow> w = BrokeAck"
proof
  assume *: "optimal bett (\<lambda>w. \<not> kept w) w"
  hence nk: "\<not> kept w" by (simp add: optimal_def)
  from * have "\<not> (\<exists>v. \<not> kept v \<and> wrank w < wrank v)"
    by (auto simp: optimal_def pstrict_bett)
  hence "\<not> (\<not> kept BrokeAck \<and> wrank w < wrank BrokeAck)" by blast
  with nk show "w = BrokeAck" by (cases w) auto
next
  assume "w = BrokeAck"
  thus "optimal bett (\<lambda>w. \<not> kept w) w"
    by (auto simp: optimal_def pstrict_bett dest!: broke_le2)
qed

subsection \<open>The four facts that monadic SDL gets wrong\<close>

text \<open>\<^bold>\<open>(1) Primary duty.\<close> You ought to keep your word (it holds in all the best worlds).\<close>
lemma primary_duty: "cobl bett kept (\<lambda>_. True)"
  unfolding cobl_def
proof (intro allI impI)
  fix w assume "optimal bett (\<lambda>_. True) w"
  hence "wrank w = 3" by (simp add: optimal_top)
  thus "kept w" by (cases w) auto
qed

text \<open>\<^bold>\<open>(2) Contrary-to-duty.\<close> GIVEN the breach, you ought to acknowledge / repair —
  the very thing the monadic operator cannot say. This is the formal seat of apology.\<close>
lemma ctd_repair: "cobl bett ack (\<lambda>w. \<not> kept w)"
  unfolding cobl_def
proof (intro allI impI)
  fix w assume "optimal bett (\<lambda>w. \<not> kept w) w"
  hence "w = BrokeAck" by (simp add: optimal_broke)
  thus "ack w" by simp
qed

text \<open>\<^bold>\<open>(3) No collapse.\<close> Acknowledgment is \<^emph>\<open>not\<close> obligatory unconditionally — the
  gentle-murderer trap avoided. (Contrast: monadic \<open>O\<langle>ack\<rangle>\<close> would force exactly this.)\<close>
lemma no_unconditional_ack: "\<not> cobl bett ack (\<lambda>_. True)"
proof -
  have "optimal bett (\<lambda>_. True) KeptSilent" by (simp add: optimal_top)
  moreover have "\<not> ack KeptSilent" by simp
  ultimately show ?thesis by (auto simp: cobl_def)
qed

text \<open>\<^bold>\<open>(4) Repair is owed by the breach, not in general.\<close> If you DID keep your word,
  no acknowledgment is required. So the duty in (2) is genuinely \<^emph>\<open>conditional\<close>.\<close>
lemma kept_needs_no_ack: "\<not> cobl bett ack kept"
proof -
  have "\<not> (\<exists>v. kept v \<and> pstrict bett v KeptSilent)"
  proof
    assume "\<exists>v. kept v \<and> pstrict bett v KeptSilent"
    then obtain v where "wrank KeptSilent < wrank v" by (auto simp: pstrict_bett)
    with wrank_le3[of v] show False by simp
  qed
  hence "optimal bett kept KeptSilent" by (simp add: optimal_def)
  moreover have "\<not> ack KeptSilent" by simp
  ultimately show ?thesis by (auto simp: cobl_def)
qed

text \<open>
  \<^bold>\<open>The point, in the system's own voice.\<close> The Chisholm/repair set is jointly
  satisfiable — the four-world model above \<^emph>\<open>is\<close> the witness — so dyadic obligation
  \<^bold>\<open>hosts contrary-to-duty without explosion\<close>, where @{theory_text Mediator}'s
  monadic \<open>O\<langle>_\<rangle>\<close> cannot. The repair obligation (2) carries positive force precisely
  in the breach worlds (it is empty given a kept promise, (4)), and never becomes the
  unconditional demand (3) that the context-free operator would manufacture.

  For the kernel this is the seam to target next: a dispute over a \<^emph>\<open>broken
  expectation\<close> reduces to (a) a conditional obligation the prover can certify is
  \<^emph>\<open>coherent\<close> and \<^emph>\<open>conditional\<close>, and (b) the contested predicate — \<^emph>\<open>was the duty
  in fact breached? what counts as repair?\<close> — which is handed back to the humans,
  undecided, exactly as the ledger crux is. The machine learns to \<^emph>\<open>see\<close> repair
  without presuming to \<^emph>\<open>rule\<close> it.
\<close>

end
