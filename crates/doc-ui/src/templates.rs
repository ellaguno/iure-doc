//! Plantillas de diagramas Mermaid (las 21 de IureEditor 1.x, con el texto de ejemplo en el idioma
//! de la interfaz). Las usa `/diagrama` y el selector «Plantilla» de un bloque ```` ```mermaid ````.

use iurefficient_ui::lang::pick;

/// (nombre, código) de cada plantilla, en el orden de la 1.x.
pub fn mermaid() -> Vec<(&'static str, String)> {
    let es = pick("en", "es") == "es";
    let t = |en: &'static str, es_: &'static str| if es { es_ } else { en };
    vec![
        (
            t("Flowchart", "Diagrama de flujo"),
            format!(
                "flowchart TD\n    A[{}] --> B{{{}}}\n    B -->|{}| C[{}]\n    B -->|{}| D[{}]",
                t("Start", "Inicio"),
                t("Decision", "Decisión"),
                t("Yes", "Sí"),
                t("Action", "Acción"),
                t("No", "No"),
                t("End", "Fin")
            ),
        ),
        (
            t("Sequence", "Secuencia"),
            format!(
                "sequenceDiagram\n    participant A as {}\n    participant B as {}\n    A->>B: {}\n    B-->>A: {}",
                t("Alice", "Ana"),
                t("Bob", "Beto"),
                t("Request", "Solicitud"),
                t("Response", "Respuesta")
            ),
        ),
        (
            t("Class", "Clases"),
            format!(
                "classDiagram\n    class {a} {{\n      +String {n}\n      +{e}()\n    }}\n    {a} <|-- {p}",
                a = t("Animal", "Animal"),
                n = t("name", "nombre"),
                e = t("eat", "comer"),
                p = t("Dog", "Perro")
            ),
        ),
        (
            t("State", "Estados"),
            format!(
                "stateDiagram-v2\n    [*] --> {i}\n    {i} --> {a}: {s}\n    {a} --> [*]: {f}",
                i = t("Idle", "Inactivo"),
                a = t("Active", "Activo"),
                s = t("start", "iniciar"),
                f = t("finish", "terminar")
            ),
        ),
        (
            t("Entity–relationship", "Entidad–relación"),
            format!(
                "erDiagram\n    {c} ||--o{{ {o} : {p}\n    {o} ||--|{{ {l} : {k}",
                c = t("CUSTOMER", "CLIENTE"),
                o = t("ORDER", "PEDIDO"),
                p = t("places", "hace"),
                l = t("LINE", "LINEA"),
                k = t("contains", "contiene")
            ),
        ),
        (
            "Gantt",
            format!(
                "gantt\n    title {}\n    dateFormat YYYY-MM-DD\n    section {}\n    {} :a1, 2024-01-01, 7d\n    {} :after a1, 5d",
                t("Plan", "Plan"),
                t("Phase 1", "Fase 1"),
                t("Task A", "Tarea A"),
                t("Task B", "Tarea B")
            ),
        ),
        (
            t("Pie", "Pastel"),
            format!("pie title {}\n    \"A\" : 40\n    \"B\" : 35\n    \"C\" : 25", t("Distribution", "Distribución")),
        ),
        (
            t("Quadrant chart", "Cuadrantes"),
            format!(
                "quadrantChart\n    title {}\n    x-axis {} --> {}\n    y-axis {} --> {}\n    quadrant-1 {}\n    quadrant-2 {}\n    quadrant-3 {}\n    quadrant-4 {}\n    {} A: [0.3, 0.6]\n    {} B: [0.45, 0.23]\n    {} C: [0.57, 0.69]\n    {} D: [0.78, 0.34]",
                t("Reach and engagement", "Alcance y participación"),
                t("Low reach", "Poco alcance"),
                t("High reach", "Mucho alcance"),
                t("Low engagement", "Poca participación"),
                t("High engagement", "Mucha participación"),
                t("Expand", "Ampliar"),
                t("Promote", "Promover"),
                t("Re-evaluate", "Reevaluar"),
                t("Improve", "Mejorar"),
                t("Campaign", "Campaña"),
                t("Campaign", "Campaña"),
                t("Campaign", "Campaña"),
                t("Campaign", "Campaña")
            ),
        ),
        (
            t("Mind map", "Mapa mental"),
            format!(
                "mindmap\n  root(({}))\n    {} A\n      {} 1\n      {} 2\n    {} B\n    {} C",
                t("Topic", "Tema"),
                t("Idea", "Idea"),
                t("Detail", "Detalle"),
                t("Detail", "Detalle"),
                t("Idea", "Idea"),
                t("Idea", "Idea")
            ),
        ),
        (
            t("Timeline", "Línea de tiempo"),
            format!(
                "timeline\n    title {}\n    2022 : {}\n    2023 : {}\n         : {}\n    2024 : {}",
                t("History", "Historia"),
                t("Founding", "Fundación"),
                t("First product", "Primer producto"),
                t("First customers", "Primeros clientes"),
                t("Expansion", "Expansión")
            ),
        ),
        (
            t("User journey", "Recorrido de usuario"),
            format!(
                "journey\n    title {}\n    section {}\n      {}: 5: {me}\n      {}: 2: {me}\n    section {}\n      {}: 3: {me}, {}\n      {}: 4: {me}",
                t("A working day", "Un día de trabajo"),
                t("Morning", "Mañana"),
                t("Make coffee", "Hacer café"),
                t("Commute", "Trayecto"),
                t("Office", "Oficina"),
                t("Meetings", "Reuniones"),
                t("Team", "Equipo"),
                t("Write code", "Redactar"),
                me = t("Me", "Yo")
            ),
        ),
        (
            t("Git graph", "Gráfico de Git"),
            "gitGraph\n    commit\n    branch feature\n    checkout feature\n    commit\n    commit\n    checkout main\n    merge feature\n    commit".to_string(),
        ),
        (
            t("XY chart (bars/lines)", "Gráfica XY (barras/líneas)"),
            format!(
                "xychart-beta\n    title \"{}\"\n    x-axis [{}]\n    y-axis \"{}\" 0 --> 100\n    bar [30, 45, 60, 52, 78, 90]\n    line [30, 45, 60, 52, 78, 90]",
                t("Monthly sales", "Ventas por mes"),
                t("Jan, Feb, Mar, Apr, May, Jun", "Ene, Feb, Mar, Abr, May, Jun"),
                t("Units", "Unidades")
            ),
        ),
        (
            "Sankey",
            format!(
                "sankey-beta\n\n{i},{s},50\n{i},{o},30\n{i},{a},20\n{o},{r},18\n{o},{v},12",
                i = t("Income", "Ingresos"),
                s = t("Salaries", "Sueldos"),
                o = t("Operations", "Operación"),
                a = t("Savings", "Ahorro"),
                r = t("Rent", "Renta"),
                v = t("Services", "Servicios")
            ),
        ),
        (
            "Kanban",
            format!(
                "kanban\n  todo[{}]\n    t1[{}]\n    t2[{}]\n  doing[{}]\n    t3[{}]\n  done[{}]\n    t4[{}]",
                t("To do", "Por hacer"),
                t("Draft contract", "Redactar contrato"),
                t("Review clauses", "Revisar cláusulas"),
                t("In progress", "En curso"),
                t("Client meeting", "Junta con el cliente"),
                t("Done", "Hecho"),
                t("Sign NDA", "Firmar NDA")
            ),
        ),
        (
            t("Block diagram", "Diagrama de bloques"),
            format!(
                "block-beta\n  columns 3\n  A[\"{}\"] space B[\"{}\"]\n  space:3\n  C[\"{}\"]:3\n  A --> B\n  B --> C",
                t("Input", "Entrada"),
                t("Process", "Proceso"),
                t("Output", "Salida")
            ),
        ),
        (
            t("Requirement", "Requisitos"),
            format!(
                "requirementDiagram\n    requirement access {{\n      id: 1\n      text: {}\n      risk: high\n      verifymethod: test\n    }}\n    element login_form {{\n      type: component\n    }}\n    login_form - satisfies -> access",
                t("Users must log in", "Los usuarios deben iniciar sesión")
            ),
        ),
        (
            t("Packet", "Paquete (bits)"),
            format!(
                "packet-beta\n  0-15: \"{}\"\n  16-31: \"{}\"\n  32-63: \"{}\"\n  64-95: \"{}\"",
                t("Source port", "Puerto de origen"),
                t("Destination port", "Puerto de destino"),
                t("Sequence number", "Número de secuencia"),
                t("Acknowledgment number", "Número de acuse")
            ),
        ),
        (
            "Radar",
            format!(
                "radar-beta\n  title {}\n  axis a[\"{}\"], b[\"{}\"], c[\"{}\"], d[\"{}\"], e[\"{}\"]\n  curve x[\"{}\"]{{4, 5, 3, 2, 4}}\n  curve y[\"{}\"]{{3, 3, 5, 4, 2}}\n  max 5",
                t("Skills", "Habilidades"),
                t("Research", "Investigación"),
                t("Drafting", "Redacción"),
                t("Negotiation", "Negociación"),
                t("Litigation", "Litigio"),
                t("Advice", "Asesoría"),
                t("Alice", "Ana"),
                t("Bob", "Beto")
            ),
        ),
        (
            t("Treemap", "Mapa de árbol"),
            format!(
                "treemap-beta\n\"{}\"\n    \"{}\"\n        \"{}\": 40\n        \"{}\": 20\n    \"{}\"\n        \"{}\": 80\n        \"{}\": 10",
                t("Budget", "Presupuesto"),
                t("Operations", "Operación"),
                t("Rent", "Renta"),
                t("Services", "Servicios"),
                t("Staff", "Personal"),
                t("Salaries", "Sueldos"),
                t("Training", "Capacitación")
            ),
        ),
        (
            t("C4 context", "Contexto C4"),
            format!(
                "C4Context\n    title {}\n    Person(user, \"{}\", \"{}\")\n    System(app, \"{}\", \"{}\")\n    System_Ext(mail, \"{}\", \"{}\")\n    Rel(user, app, \"{}\")\n    Rel(app, mail, \"{}\")",
                t("System context", "Contexto del sistema"),
                t("Client", "Cliente"),
                t("Uses the system", "Usa el sistema"),
                t("Application", "Aplicación"),
                t("Manages cases", "Gestiona los casos"),
                t("Email", "Correo"),
                t("Sends notices", "Envía avisos"),
                t("Uses", "Usa"),
                t("Sends emails", "Envía correos")
            ),
        ),
    ]
}

/// El código de la primera plantilla (diagrama de flujo).
pub(crate) fn first() -> String {
    mermaid().into_iter().next().map(|(_, c)| c).unwrap_or_default()
}

/// ¿Es un bloque ```` ```mermaid ````?
pub(crate) fn is_mermaid(block: &str) -> bool {
    let t = block.trim_start();
    (t.starts_with("```") || t.starts_with("~~~"))
        && t.lines().next().is_some_and(|l| l.trim_start_matches(['`', '~']).trim().eq_ignore_ascii_case("mermaid"))
}

/// El bloque con otro código (conserva la cerca).
pub(crate) fn replace_code(block: &str, code: &str) -> String {
    let t = block.trim_start();
    let fence: String = t.chars().take_while(|c| *c == '`' || *c == '~').collect();
    let fence = if fence.len() < 3 { "```".to_string() } else { fence };
    format!("{fence}mermaid\n{code}\n{fence}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_complete_and_blocks_keep_their_fence() {
        iurefficient_ui::lang::set_lang(iurefficient_ui::lang::Lang::Es);
        let all = mermaid();
        assert_eq!(all.len(), 21);
        assert_eq!(all.first().map(|x| x.0), Some("Diagrama de flujo"));
        assert!(first().starts_with("flowchart TD\n    A[Inicio]"));
        assert!(is_mermaid("```mermaid\nx\n```") && is_mermaid("~~~ Mermaid\nx\n~~~"));
        assert!(!is_mermaid("```rust\nx\n```"));
        assert_eq!(replace_code("````mermaid\nviejo\n````", "pie"), "````mermaid\npie\n````");
    }
}
