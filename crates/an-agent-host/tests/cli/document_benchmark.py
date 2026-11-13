"""Frozen synthetic document inputs, domain checks and disclosure fixtures."""
import hashlib
import json
from pathlib import Path


def expected_rows(scenario):
    quantity = 13 if scenario.endswith('table_revision') else 12
    return [('Bolt', quantity, .5), ('Plate', 3, 8), ('Bearing', 2, 4.25)]


def make_pdf(path, scenario):
    lines = ['Workshop supplies', 'Currency EUR', 'Item Quantity Unit price']
    ambiguous = scenario.endswith('table_ambiguous')
    variation = scenario.endswith('schema_variation')
    credits = scenario.startswith('context_pressure.')
    if ambiguous:
        lines[1] = 'Currency not specified'
    if variation:
        lines[2] = 'Part Count Unit cost (EUR cents)'
    if credits:
        lines[1:3] = ['Prices in workshop credits; output required in EUR', 'Item Quantity Unit price (credits)']
    for name, quantity, price in expected_rows(scenario):
        if ambiguous and name == 'Bolt':
            lines.append(f'{name} ? {price:.2f}')
            continue
        if variation:
            price *= 100
        if credits:
            price *= 4
        if scenario.endswith('table_revision') and name == 'Bearing':
            lines.extend(['Bear', f'ing {quantity} {price:.2f}'])
        else:
            lines.append(f'{name} {quantity} {price:.2f}')
    content = 'BT /F1 12 Tf 40 780 Td ' + ' '.join(
        ('0 -20 Td ' if i else '') + '(' + line + ') Tj' for i, line in enumerate(lines)) + ' ET'
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>',
               b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
               b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 842] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',
               b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
               f'<< /Length {len(content)} >>\nstream\n{content}\nendstream'.encode()]
    data = bytearray(b'%PDF-1.4\n')
    offsets = [0]
    for i, body in enumerate(objects, 1):
        offsets.append(len(data))
        data.extend(f'{i} 0 obj\n'.encode() + body + b'\nendobj\n')
    xref = len(data)
    data.extend(b'xref\n0 6\n0000000000 65535 f \n')
    for offset in offsets[1:]:
        data.extend(f'{offset:010d} 00000 n \n'.encode())
    data.extend(f'trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode())
    Path(path).write_bytes(data)


def knowledge(root, scenario):
    folder = root / 'knowledge'
    folder.mkdir()
    permissions, hashes = [], []

    def material(name, text):
        path = folder / name
        path.write_text(text)
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        permissions.append({'kind': 'read-input', 'path': str(path), 'sha256': digest})
        hashes.append(digest)
        return {'op': 'read-input', 'path': str(path), 'sha256': digest, 'encoding': 'utf-8'}

    pdf_skill = material('pdf-skill.txt',
        'Preserve every source row. Join wrapped labels only when the adjacent numeric columns '
        'identify the same row. Keep quantities as integers and EUR prices as decimal values. '
        'Use Item | Quantity | Unit price. Do not guess missing values. '
        'Convert EUR cents by dividing by 100. '
        + ('This selected release defines one workshop credit as EUR 0.25.'
           if scenario.startswith('context_pressure.') else ''))
    sheet_skill = material('sheet-skill.txt',
        'Read the accepted Markdown revision. Extract its rows in source order. '
        'The workbook authoring library writes editable numeric cells, line formulas and a SUM total. '
        'Return item, integer quantity and numeric unit_price. Preserve EUR values.')
    selected = 'document-table'
    if scenario.endswith('unseen_spool'):
        selected = 'capability-' + hashlib.sha256(scenario.encode()).hexdigest()[:12]
    contract = material('contract.json', json.dumps({'id': selected, 'version': '1.0.0',
        'input': 'PDF selectable text', 'output': ['Markdown table', 'editable XLSX'],
        'skills': [pdf_skill], 'sheet_skill': sheet_skill, 'effects': 'bounded proposals for host acceptance'}))
    count = int(scenario.rsplit('_', 1)[1]) if scenario.startswith('spool_selection.') else 4
    cards = [{'id': f'export-{i}', 'purpose': 'Export a table as a flattened PDF image',
              'input': 'table', 'output': ['PDF'], 'eligible_for_editable_workbook': False}
             for i in range(count - 1)]
    cards.append({'id': selected, 'purpose': 'Preserve PDF tables through Markdown and editable workbooks',
                  'input': 'PDF selectable text', 'output': ['Markdown', 'XLSX'], 'contract': contract})
    irrelevant = 16384 if scenario.endswith('knowledge_medium') else 65536 if scenario.endswith('knowledge_large') else 0
    if irrelevant:
        obsolete = material('unrelated-release.txt', ('Obsolete release: one workshop credit equals EUR 1.00.\n' * 1500)[:irrelevant])
        cards.insert(0, {'id': 'obsolete-unrelated', 'eligible': False, 'knowledge': obsolete})
    next_page = None
    for start in reversed(range(0, len(cards), 16)):
        next_page = material(f'index-{start // 16}.json', json.dumps({'cards': cards[start:start + 16], 'next': next_page}))
    return {'permissions': permissions, 'knowledge_hashes': hashes,
            'required_hashes': [pdf_skill['sha256'], sheet_skill['sha256'], contract['sha256']],
            'irrelevant_hashes': [obsolete['sha256']] if irrelevant else [], 'index': next_page,
            'sheet': sheet_skill, 'selected': selected}


def write_workbook(source, destination):
    import xlsxwriter
    with xlsxwriter.Workbook(destination) as book:
        sheet = book.add_worksheet('Supplies')
        sheet.write('A2', 'Workshop supplies')
        sheet.write('A3', 'EUR. Source is the accepted Markdown revision.')
        sheet.write_row('A5', ['Item', 'Quantity', 'Unit price (EUR)', 'Amount (EUR)'])
        total = 0
        for index, row in enumerate(source['rows'], 6):
            sheet.write_row(index - 1, 0, [row['item'], row['quantity'], row['unit_price']])
            amount = row['quantity'] * row['unit_price']
            sheet.write_formula(f'D{index}', f'=B{index}*C{index}', None, amount)
            total += amount
        sheet.write('A10', 'Total (EUR)')
        sheet.write_formula('D10', '=SUM(D6:D8)', None, total)
        sheet.write('A12', 'Markdown SHA256 ' + source['source_sha256'])
        sheet.set_column('A:A', 28)
        sheet.set_column('B:D', 23)
