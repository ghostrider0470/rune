import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { A2uiComponent } from "@/lib/api-types";

export function A2uiTable({ component }: { component: A2uiComponent }) {
  const headers = Array.isArray(component.headers) ? component.headers : [];
  const rows = Array.isArray(component.rows) ? component.rows : [];

  return (
    <div className="rounded-md border">
      <Table>
        {headers.length > 0 && (
          <TableHeader>
            <TableRow>
              {headers.map((h, i) => (
                <TableHead key={i}>{String(h)}</TableHead>
              ))}
            </TableRow>
          </TableHeader>
        )}
        <TableBody>
          {rows.map((row, ri) => (
            <TableRow key={ri}>
              {(Array.isArray(row) ? row : [row]).map((cell, ci) => (
                <TableCell key={ci}>{String(cell)}</TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
