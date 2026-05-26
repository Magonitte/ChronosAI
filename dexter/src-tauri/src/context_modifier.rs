//! context_modifier.rs — Tier 1 (scaffolding)
//! Detecta modificadores de contexto que alteram o system prompt da voz.

// ---------------------------------------------------------------------------
// Tipos
// ---------------------------------------------------------------------------

/// Modificador de contexto: ajusta o system prompt conforme o conteúdo detectado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextModifier {
    /// Clipboard contém stack trace / mensagem de erro.
    ErrorDiagnosis,
    /// Clipboard contém código-fonte (heurística).
    CodeReview,
    /// Usuário quer resposta rápida e concisa.
    ConciseResponse,
}

// ---------------------------------------------------------------------------
// Detecção
// ---------------------------------------------------------------------------

/// Detecta modificadores de contexto com base na transcrição e no conteúdo do clipboard.
pub fn detect_modifiers(transcript: &str, clipboard: &str) -> Vec<ContextModifier> {
    let mut mods: Vec<ContextModifier> = Vec::new();
    let t = transcript.to_lowercase();

    // ── Resposta concisa ──
    if t.contains("rapido")
        || t.contains("rapidamente")
        || t.contains("resumido")
        || t.contains("resumindo")
        || t.contains("em resumo")
        || t.contains("breve")
        || t.contains("brevemente")
        || t.contains("em poucas palavras")
        || t.contains("de forma curta")
        || t.contains("curto")
    {
        mods.push(ContextModifier::ConciseResponse);
    }

    // ── Diagnóstico de erro / revisão de código (via clipboard) ──
    if !clipboard.is_empty() {
        let clip = clipboard.to_lowercase();

        let looks_like_error = clip.contains("error:")
            || clip.contains("exception")
            || clip.contains("traceback (most recent call last)")
            || clip.contains("stack trace")
            || clip.contains("at line")
            || clip.contains("panicked at")
            || clip.contains("thread 'main' panicked")
            || clip.contains("fatal error")
            || clip.contains("errno")
            || (clip.contains("error") && clip.contains("line "))
            || (clip.contains("erro") && (t.contains("erro") || t.contains("error") || t.contains("explica") || t.contains("resolve") || t.contains("diagnostica")));

        if looks_like_error {
            mods.push(ContextModifier::ErrorDiagnosis);
        } else {
            let looks_like_code = clip.contains("fn ")
                || clip.contains("pub fn")
                || clip.contains("async fn")
                || clip.contains("function ")
                || clip.contains("def ")
                || clip.contains("class ")
                || clip.contains("import ")
                || clip.contains("use ")
                || clip.contains("const ")
                || clip.contains("return ")
                || clip.contains("if (")
                || clip.contains("for (")
                || clip.contains("while (")
                || clip.contains("->")
                || clip.contains("=>");

            let user_wants_review = t.contains("explica")
                || t.contains("revisa")
                || t.contains("review")
                || t.contains("analisa")
                || t.contains("codigo")
                || t.contains("esse codigo")
                || t.contains("esse script")
                || t.contains("o que faz");

            if looks_like_code && user_wants_review {
                mods.push(ContextModifier::CodeReview);
            }
        }
    }

    mods
}

// ---------------------------------------------------------------------------
// Mapeamento para system prompt
// ---------------------------------------------------------------------------

/// Retorna instrução adicional de system prompt para um modificador.
pub fn modifier_to_prompt(modifier: &ContextModifier) -> &'static str {
    match modifier {
        ContextModifier::ErrorDiagnosis => {
            "O usuário compartilhou um erro. Diagnostique a causa raiz de forma direta em até 3 frases. Sugira a correção mais provável."
        }
        ContextModifier::CodeReview => {
            "O usuário compartilhou código. Revise de forma concisa, destacando apenas os pontos mais importantes ou problemáticos."
        }
        ContextModifier::ConciseResponse => {
            "Responda em no máximo 1-2 frases curtas."
        }
    }
}
