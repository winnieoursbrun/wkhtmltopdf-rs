#!/usr/bin/env python3
"""Generate the Rails-like corpus pages (invoice, report, fonts, layout, statement).

Images and the web font are inlined as data: URIs, as wicked_pdf/pdfkit apps commonly do:
pdfkit feeds the HTML on stdin, so the page has no base URL and wkhtmltopdf 0.12.6 blocks
file:// access by default.
"""
import base64
import os
import random

HERE = os.path.dirname(os.path.abspath(__file__))
LOGO = "data:image/png;base64," + open(os.path.join(HERE, "logo.b64")).read().strip()
FONT = "data:font/ttf;base64," + base64.b64encode(
    open(os.path.join(HERE, "..", "..", "crates", "wk-fonts", "fonts", "DejaVuSansMono-Bold.ttf"), "rb").read()).decode()

rnd = random.Random(42)
WORDS = ("facture client règlement échéance réalisé prestation matériel câblage intervention "
         "dépannage contrôle sécurité vérification électrique façade bâtiment système à la "
         "suite de du des pour les une avec être été très leçon garçon où même déjà Noël "
         "naïve cœur œuvre équipe société maître hôtel forêt").split()


def sentence(n):
    w = [rnd.choice(WORDS) for _ in range(n)]
    return " ".join(w).capitalize() + "."


def para(ns=6):
    return " ".join(sentence(rnd.randint(8, 16)) for _ in range(ns))


def write(name, html):
    with open(os.path.join(HERE, name), "w", encoding="utf-8") as f:
        f.write(html)


# 1. Invoice: floats header, logo, thead repeated over 2+ pages, totals, accents.
rows = []
total = 0
for i in range(1, 66):
    qty = rnd.randint(1, 12)
    pu = rnd.randint(500, 25000) / 100
    total += qty * pu
    rows.append(f"<tr><td>REF-{i:04d}</td><td>{sentence(rnd.randint(3, 7))[:-1]}</td>"
                f"<td class='num'>{qty}</td><td class='num'>{pu:.2f} €</td>"
                f"<td class='num'>{qty * pu:.2f} €</td></tr>")
write("invoice.html", f"""<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Facture FA-2026-0142</title>
<style>
body {{ font-family: "DejaVu Sans", Arial, sans-serif; font-size: 11px; color: #222; }}
.header {{ overflow: hidden; border-bottom: 2px solid #1e3c8c; padding-bottom: 8px; margin-bottom: 16px; }}
.logo {{ float: left; width: 120px; height: 40px; }}
.company {{ float: right; text-align: right; line-height: 1.4; }}
.addresses {{ overflow: hidden; margin-bottom: 20px; }}
.from {{ float: left; width: 45%; }}
.to {{ float: right; width: 45%; border: 1px solid #ccc; padding: 8px; background: #f5f5f5; }}
h1 {{ font-size: 20px; color: #1e3c8c; margin: 0 0 12px 0; }}
table.lines {{ width: 100%; border-collapse: collapse; }}
table.lines thead {{ display: table-header-group; }}
table.lines th {{ background: #1e3c8c; color: #fff; text-align: left; padding: 4px 6px; }}
table.lines td {{ border-bottom: 1px solid #ddd; padding: 4px 6px; }}
.num {{ text-align: right; white-space: nowrap; }}
tr {{ page-break-inside: avoid; }}
table.totals {{ width: 40%; margin-left: 60%; margin-top: 12px; border-collapse: collapse; }}
table.totals td {{ padding: 3px 6px; }}
table.totals tr.grand td {{ font-weight: bold; border-top: 2px solid #222; font-size: 13px; }}
.note {{ margin-top: 24px; font-size: 9px; color: #666; }}
</style></head><body>
<div class="header">
  <img class="logo" src="{LOGO}" alt="logo">
  <div class="company"><strong>Modulo Énergie SAS</strong><br>12 rue de la Gare<br>69003 Lyon — France<br>SIRET 123 456 789 00012</div>
</div>
<h1>Facture n° FA-2026-0142</h1>
<div class="addresses">
  <div class="from">Date : 14/10/2026<br>Échéance : 13/11/2026<br>Référence client : CLT-0042</div>
  <div class="to"><strong>Société Générale d'Électricité</strong><br>Service comptabilité<br>4 place de l'Hôtel de Ville<br>75004 Paris</div>
</div>
<table class="lines">
<thead><tr><th>Réf.</th><th>Désignation</th><th class="num">Qté</th><th class="num">P.U. HT</th><th class="num">Total HT</th></tr></thead>
<tbody>
{chr(10).join(rows)}
</tbody>
</table>
<table class="totals">
<tr><td>Total HT</td><td class="num">{total:.2f} €</td></tr>
<tr><td>TVA 20 %</td><td class="num">{total * 0.2:.2f} €</td></tr>
<tr class="grand"><td>Total TTC</td><td class="num">{total * 1.2:.2f} €</td></tr>
</table>
<p class="note">Pénalités de retard : trois fois le taux d'intérêt légal. Indemnité forfaitaire pour frais de recouvrement : 40 €. Pas d'escompte pour paiement anticipé.</p>
</body></html>
""")

# 2. Report: cover + page-break-after, chapters with page-break-before, @media print/screen, long text.
chapters = []
for c in range(1, 4):
    body = "".join(f"<h2>{c}.{s} {sentence(4)[:-1]}</h2>" + "".join(f"<p>{para()}</p>" for _ in range(4))
                   for s in range(1, 4))
    chapters.append(f'<h1 class="chapter">Chapitre {c} — {sentence(3)[:-1]}</h1>{body}')
write("report.html", f"""<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Rapport d'intervention</title>
<style>
body {{ font-family: "DejaVu Serif", Georgia, serif; font-size: 12px; line-height: 1.5; }}
.cover {{ text-align: center; padding-top: 200px; page-break-after: always; }}
.cover h1 {{ font-size: 32px; }}
h1.chapter {{ page-break-before: always; color: #8c1e1e; border-bottom: 1px solid #8c1e1e; }}
h2 {{ font-size: 15px; margin-top: 18px; }}
p {{ text-align: justify; margin: 0 0 8px 0; }}
.print-only, .screen-only {{ padding: 6px; border: 1px dashed #999; }}
@media print {{ .screen-only {{ display: none; }} body {{ color: #000080; }} }}
@media screen {{ .print-only {{ display: none; }} }}
</style></head><body>
<div class="cover">
  <h1>Rapport d'intervention annuel</h1>
  <p style="text-align:center">Exercice 2026 — Établissement « Les Mûriers »</p>
  <p class="screen-only">SCREEN MEDIA ACTIVE</p>
  <p class="print-only">PRINT MEDIA ACTIVE</p>
</div>
{chr(10).join(chapters)}
</body></html>
""")

# 3. Fonts: @font-face from data: URI, system family, bold, synthetic italic, UTF-8 coverage.
SAMPLE = "àâäéèêëîïôöùûüç ÀÉÈÇ œŒ æÆ € £ ß ñ — « guillemets » “quotes” ‘single’ … ½ ° ± × ÷ ©"
write("fonts.html", f"""<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Fonts</title>
<style>
@font-face {{ font-family: "BrandMono"; src: url("{FONT}") format("truetype"); font-weight: normal; }}
body {{ font-family: "DejaVu Sans", sans-serif; font-size: 13px; }}
.brand {{ font-family: "BrandMono", monospace; font-size: 22px; color: #1e3c8c; }}
.serif {{ font-family: "DejaVu Serif", serif; }}
.mono {{ font-family: "DejaVu Sans Mono", monospace; }}
.small {{ font-size: 8px; }} .big {{ font-size: 28px; }}
</style></head><body>
<p class="brand">BrandMono web font — Facture 0142</p>
<p>Sans regular: {SAMPLE}</p>
<p><b>Sans bold: {SAMPLE}</b></p>
<p><i>Sans italic (synthetic): {SAMPLE}</i></p>
<p class="serif">Serif regular: {SAMPLE}</p>
<p class="serif"><b>Serif bold: {SAMPLE}</b></p>
<p class="mono">Mono: 0123456789 iIl1| O0 {{}}[]() =&gt; ->  {SAMPLE}</p>
<p class="small">Small 8px: {para(2)}</p>
<p class="big">Big 28px: Ça déménage !</p>
<p style="text-decoration: underline">Underlined text</p>
<p style="text-decoration: line-through">Struck text</p>
<p style="letter-spacing: 3px; text-transform: uppercase">letter spacing uppercase</p>
</body></html>
""")

# 4. Layout: floats with wrap, two columns, inline-block, lists, borders, absolute stamp, colspan/rowspan.
write("layout.html", f"""<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Layout</title>
<style>
body {{ font-family: "DejaVu Sans", sans-serif; font-size: 11px; position: relative; }}
.stamp {{ position: absolute; top: 10px; right: 10px; border: 3px solid #c00; color: #c00; font-size: 24px; padding: 4px 12px; transform: rotate(-12deg); }}
.wrap img {{ float: left; margin: 0 10px 6px 0; width: 120px; }}
.cols {{ overflow: hidden; margin: 12px 0; }}
.col {{ float: left; width: 48%; margin-right: 2%; background: #eef; padding: 6px; box-sizing: border-box; }}
.badge {{ display: inline-block; padding: 2px 8px; border-radius: 8px; background: #2a7; color: #fff; margin-right: 4px; }}
.box {{ border: 2px dashed #888; padding: 8px; margin: 8px 0; }}
.box2 {{ border: 3px dotted #c60; border-radius: 6px; padding: 8px; margin: 8px 0; background: #fff7ee; }}
table.grid {{ border-collapse: collapse; width: 70%; }}
table.grid td, table.grid th {{ border: 1px solid #444; padding: 4px; text-align: center; }}
.clear {{ clear: both; }}
</style></head><body>
<div class="stamp">PAYÉ</div>
<h1>Bon d'intervention</h1>
<div class="wrap"><img src="{LOGO}" alt="logo"><p>{para(5)}</p></div>
<div class="clear"></div>
<div class="cols"><div class="col"><h3>Colonne gauche</h3><p>{para(3)}</p></div><div class="col"><h3>Colonne droite</h3><p>{para(3)}</p></div></div>
<p><span class="badge">Urgent</span><span class="badge" style="background:#c33">Facturé</span><span class="badge" style="background:#36c">Clôturé</span></p>
<ul><li>Contrôle du tableau électrique</li><li>Remplacement disjoncteur différentiel</li><li>Essais et mise en service</li></ul>
<ol><li>Première étape</li><li>Deuxième étape</li><li>Troisième étape</li></ol>
<div class="box">Encadré pointillé : {sentence(12)}</div>
<div class="box2">Encadré arrondi : {sentence(12)}</div>
<table class="grid">
<tr><th rowspan="2">Poste</th><th colspan="2">Heures</th></tr>
<tr><th>Prévu</th><th>Réalisé</th></tr>
<tr><td>Électricien</td><td>8</td><td>9,5</td></tr>
<tr><td>Apprenti</td><td>8</td><td>7</td></tr>
<tr><td colspan="2">Total</td><td>16,5</td></tr>
</table>
</body></html>
""")

# 5. Statement: long striped table with thead + tfoot, then a forced-break summary.
srows = []
bal = 1000.0
for i in range(1, 121):
    amt = rnd.randint(-30000, 30000) / 100
    bal += amt
    srows.append(f"<tr><td>{(i % 28) + 1:02d}/09/2026</td><td>{sentence(rnd.randint(2, 5))[:-1]}</td>"
                 f"<td class='num'>{amt:.2f}</td><td class='num'>{bal:.2f}</td></tr>")
write("statement.html", f"""<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Relevé</title>
<style>
body {{ font-family: "DejaVu Sans", sans-serif; font-size: 10px; }}
table {{ width: 100%; border-collapse: collapse; }}
thead {{ display: table-header-group; }}
tfoot {{ display: table-footer-group; }}
th {{ background: #333; color: #fff; padding: 3px; }}
td {{ padding: 3px; border-bottom: 1px solid #ccc; }}
tbody tr:nth-child(even) {{ background: #f0f0f0; }}
.num {{ text-align: right; }}
.summary {{ page-break-before: always; }}
</style></head><body>
<h2>Relevé de compte — septembre 2026</h2>
<table>
<thead><tr><th>Date</th><th>Libellé</th><th class="num">Montant</th><th class="num">Solde</th></tr></thead>
<tfoot><tr><td colspan="4">Suite au verso — relevé n° 9</td></tr></tfoot>
<tbody>
{chr(10).join(srows)}
</tbody>
</table>
<div class="summary"><h2>Synthèse</h2><p>Solde final : {bal:.2f} €</p><p>{para(3)}</p></div>
</body></html>
""")
print("ok")
