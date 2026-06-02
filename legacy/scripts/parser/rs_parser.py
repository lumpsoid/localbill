#!/usr/bin/env python3

import sys
import argparse
import json
import re
import time
from datetime import datetime
from typing import Optional, Dict, List, Any
from urllib.parse import urlencode
import requests
from lxml import html


class ItemJson:
    def __init__(self, data: Dict[str, Any]):
        self.gtin = data.get("gtin", "")
        self.name = data.get("name", "")
        self.quantity = data.get("quantity", 0.0)
        self.total = data.get("total", 0.0)
        self.unit_price = data.get("unitPrice", 0.0)
        self.label = data.get("label", "")
        self.label_rate = data.get("labelRate", 0.0)
        self.tax_base_amount = data.get("taxBaseAmount", 0.0)
        self.vat_amount = data.get("vatAmount", 0.0)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "name": self.name,
            "total": self.total,
            "unit_price": self.unit_price,
            "quantity": self.quantity,
            "gtin": self.gtin,
            "label": self.label,
            "label_rate": self.label_rate,
            "tax_base_amount": self.tax_base_amount,
            "vat_amount": self.vat_amount,
        }


class RsParser:
    TOKEN_XPATH = "/html/head/script[9]"
    INVOICE_XPATH = "//*[@id='invoiceNumberLabel']"
    PRICE_XPATH = "//*[@id='totalAmountLabel']"
    BUY_DATE_XPATH = "//*[@id='sdcDateTimeLabel']"
    BILL_XPATH = "//*[@id='collapse3']/div/pre"
    NAME_XPATH = "//*[@id='shopFullNameLabel']"
    TOKEN_REGEX = r"viewModel\.Token\('(.*)'\);"
    DATE_LAYOUT = "%d.%m.%Y. %H:%M:%S"

    USER_AGENT = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.3"

    def __init__(self):
        self.session = requests.Session()
        self.session.headers.update({"User-Agent": RsParser.USER_AGENT})

    @staticmethod
    def clean_price(s: str) -> str:
        """Remove thousands separator and replace comma with dot for decimal"""
        return s.replace(".", "").replace(",", ".")

    @staticmethod
    def clean_whitespace(s: str) -> str:
        """Remove leading and trailing whitespace"""
        return s.strip()

    def query_node(
        self, tree: html.HtmlElement, xpath: str
    ) -> Optional[html.HtmlElement]:
        """Returns the node if found, otherwise None"""
        nodes = tree.xpath(xpath)
        return nodes[0] if nodes else None

    def fetch_items(self, tree: html.HtmlElement) -> List[Dict[str, Any]]:
        """Fetch detailed item information from the API"""
        # Get invoice number
        invoice_node = self.query_node(tree, self.INVOICE_XPATH)
        if invoice_node is None:
            raise ValueError("Invoice number not found")
        invoice_number = self.clean_whitespace(invoice_node.text_content())

        # Get token from script
        token_node = self.query_node(tree, self.TOKEN_XPATH)
        if token_node is None:
            raise ValueError("Token script not found")

        script_text = token_node.text_content()
        pattern = re.compile(self.TOKEN_REGEX)
        matches = pattern.findall(script_text)

        if not matches:
            raise ValueError("Token not found in script")

        token = matches[0]

        # Prepare POST request
        form_data = {"invoiceNumber": invoice_number, "token": token}

        headers = {"Content-Type": "application/x-www-form-urlencoded"}

        response = self.session.post(
            "https://suf.purs.gov.rs/specifications",
            data=urlencode(form_data),
            headers=headers,
            timeout=15,
        )

        if response.status_code != 200:
            raise ValueError(f"Unexpected status code: {response.status_code}")

        response_json = response.json()

        if not response_json.get("success", False):
            raise ValueError("Error fetching invoice items")

        items = []
        for item_data in response_json.get("items", []):
            item_obj = ItemJson(item_data)
            item_dict = item_obj.to_dict()
            items.append(item_dict)

        return items

    def parse_date(self, date_string: str) -> datetime:
        """Parse date string to datetime object"""
        return datetime.strptime(date_string, self.DATE_LAYOUT)

    def parse(self, url: str, max_attempts: int = 3) -> Dict[str, Any]:
        """Parse the Serbian invoice URL and return structured data"""

        for attempt in range(1, max_attempts + 1):
            if attempt > 1:
                print(f"Attempt {attempt}: Refetching page for new token")
                time.sleep(1)

            try:
                # Fetch the page
                response = self.session.get(url, timeout=15)

                if response.status_code != 200:
                    print(
                        f"Attempt {attempt}: Unexpected status {response.status_code}"
                    )
                    if attempt == max_attempts:
                        raise ValueError(f"Bad response: {response.status_code}")
                    continue

                # Parse HTML
                tree = html.fromstring(response.content)

                # Only parse static fields on first attempt
                if attempt == 1:
                    # Extract all required fields
                    required_fields = {
                        "invoice_number": self.INVOICE_XPATH,
                        "price": self.PRICE_XPATH,
                        "buy_date": self.BUY_DATE_XPATH,
                        "bill": self.BILL_XPATH,
                        "name": self.NAME_XPATH,
                    }

                    nodes = {}
                    missing_fields = []

                    for field_name, xpath in required_fields.items():
                        node = self.query_node(tree, xpath)
                        if node is not None:
                            nodes[field_name] = node
                        else:
                            missing_fields.append(f"{field_name} (XPath: {xpath})")

                    if missing_fields:
                        # Raise an error that contains the full list of missing items
                        error_msg = "Missing required fields: " + ", ".join(
                            missing_fields
                        )
                        raise ValueError(error_msg)

                    # Extract text content
                    invoice_number = self.clean_whitespace(nodes["invoice_number"].text_content())
                    date_string = self.clean_whitespace(nodes["buy_date"].text_content())
                    price_string = self.clean_whitespace(
                        self.clean_price(nodes["price"].text_content())
                    )
                    bill_text = nodes["bill"].text_content()
                    shop_name = nodes["name"].text_content()

                    # Parse date
                    date_time = self.parse_date(date_string)

                    # Parse price
                    price = float(price_string)

                # Try to fetch items
                items = self.fetch_items(tree)

                # Success
                result = {
                    "success": True,
                    "invoice_number": invoice_number,
                    "retailer": shop_name,
                    "date": date_time.isoformat(),
                    "total_price": price,
                    "currency": "RSD",
                    "country": "Serbia",
                    "url": url,
                    "raw_bill_text": bill_text,
                    "items": items,
                }

                return result

            except ValueError as e:
                if "Error fetching invoice items" in str(e):
                    print(f"Attempt {attempt}: Failed to fetch items, will retry")
                    if attempt == max_attempts:
                        raise ValueError(
                            f"Failed to fetch items after {max_attempts} attempts"
                        )
                    continue
                else:
                    raise

            except Exception as e:
                print(f"Attempt {attempt}: Error - {e}")
                if attempt == max_attempts:
                    raise

        raise ValueError("Failed to parse invoice")


def main():
    parser_arg = argparse.ArgumentParser(description="Parse invoice data from a URL.")

    parser_arg.add_argument("url", help="The URL of the invoice to be parsed")
    parser_arg.add_argument(
        "--pretty-print",
        action="store_true",
        help="Format the JSON output with indentation",
    )

    args = parser_arg.parse_args()

    url = args.url.strip()
    if not url:
        print("Error: <invoice_url> cannot be empty.")
        sys.exit(1)

    indent_val = 2 if args.pretty_print else None

    parser = RsParser()

    try:
        result = parser.parse(url)

        print(json.dumps(result, ensure_ascii=False, indent=indent_val))

    except Exception as e:
        error_result = {"error": str(e), "success": False}
        print(json.dumps(error_result, indent=2))
        sys.exit(1)


if __name__ == "__main__":
    main()
