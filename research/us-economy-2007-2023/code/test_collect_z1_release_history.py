import hashlib
from pathlib import Path
import tempfile
import unittest

import collect_z1_release_history as collection


def calendar_row(date, quarter, csv=False):
    csv_link = f'<a href="/releases/z1/{date}/z1_csv_files.zip">CSV</a>' if csv else ''
    return (f'<div class="row"><div>{quarter}</div><div>'
            f'<a href="/releases/z1/{date}/z1.pdf">PDF</a>{csv_link}</div></div>')


class ReleaseCollectionTests(unittest.TestCase):
    def setUp(self):
        self.rows = [calendar_row(date, quarter) for date, quarter in (
            ('20061207', '2006:Q3'), ('20070308', '2006:Q4'),
            ('20070607', '2007:Q1'), ('20070917', '2007:Q2'))]

    def test_nested_calendar_rows_are_not_duplicate_releases(self):
        html = '<div class="row">' + ''.join(reversed(self.rows)) + '</div>'
        records = collection.parse_calendar(html, '2006:Q3', '2007:Q2')
        self.assertEqual([row['release_path_id'] for row in records],
                         ['20061207', '20070308', '20070607', '20070917'])

    def test_missing_quarter_and_conflicting_release_fail(self):
        with self.assertRaisesRegex(ValueError, 'missing reporting quarter'):
            collection.parse_calendar(''.join(self.rows[:2] + self.rows[3:]), '2006:Q3', '2007:Q2')
        with self.assertRaisesRegex(ValueError, 'conflicting releases'):
            collection.parse_calendar(''.join(self.rows) + calendar_row('20070309', '2006:Q4'),
                                      '2006:Q3', '2007:Q2')

    def test_commented_download_variants_are_not_links(self):
        parser = collection.Links()
        parser.feed('<a href="Disk/ltabs.zip">PRN</a><!-- <a href="Disk/ltabs2.zip">CSV</a> -->')
        self.assertEqual(parser.links, ['Disk/ltabs.zip'])

    def test_publication_date_comes_from_cover_not_url_path(self):
        identity = collection.cover_identity(collection.DESTINATION / '20170305/z1.pdf')
        self.assertEqual(identity, {'publication_date': '2017-03-09', 'reported_through': '2016:Q4'})

    def test_redirect_to_current_or_other_release_is_rejected(self):
        collection.require_official_url('https://www.federalreserve.gov/releases/z1/20070308/z1.pdf', '20070308')
        for url in (
            'https://www.federalreserve.gov/releases/z1/current/z1.pdf',
            'https://www.federalreserve.gov/releases/z1/20230309/z1.pdf',
            'https://example.org/releases/z1/20070308/z1.pdf',
            'http://www.federalreserve.gov/releases/z1/20070308/z1.pdf',
        ):
            with self.subTest(url=url), self.assertRaises(ValueError):
                collection.require_official_url(url, '20070308')

    def test_cached_download_must_have_matching_receipt(self):
        url = 'https://www.federalreserve.gov/releases/z1/20070308/z1.pdf'
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'z1.pdf'
            path.write_bytes(b'%PDF-fixture')
            receipt = {'source_url': url, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
            self.assertEqual(collection.fetch(url, path, 'pdf', release='20070308', previous=receipt), receipt)
            path.write_bytes(b'%PDF-changed')
            with self.assertRaisesRegex(ValueError, 'matching receipt'):
                collection.fetch(url, path, 'pdf', release='20070308', previous=receipt)


if __name__ == '__main__':
    unittest.main()
